//! The account API's connections (M4 hosting spec §3.10; Ruling HP9). Both of its listeners, plain
//! and TLS, accept every connection, then give it one of the open connections' places, counted
//! by the peer's network as the gate counts its own (M3 spec D17), or close it at once; and they
//! hand axum each one in a [`Conn`], which keeps its place until it closes and ends it once no
//! byte has come in or gone out for the idle time, or a request has taken longer than the request
//! time. axum gives hyper no timer, so hyper's own limit on reading a request's head never
//! applies: without these, connections that send nothing, a byte now and then, or never read
//! their answers, would stay open until the process ran out of file descriptors, for the gate as
//! for the API. Out of descriptors anyway, the listeners' accept is axum's, which waits a second
//! between tries.
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Instant, Sleep};

use crate::gate::{Place, Places};

/// How many connections the account API keeps open, and for how long one may send nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnLimits {
    /// Connections open at once, TLS handshakes among them.
    pub open: usize,
    /// Of those, from one network: an IPv4 address or an IPv6 /64. A trusted proxy's count toward
    /// `open` alone.
    pub open_per_network: usize,
    /// No byte in or out of a connection for this long closes it, whatever it was waiting for: a
    /// request from its client, or its client to read an answer.
    pub idle: Duration,
    /// A request must come in whole within this long of its first byte.
    pub request: Duration,
}

impl Default for ConnLimits {
    fn default() -> Self {
        Self {
            open: 256,
            open_per_network: 16,
            idle: Duration::from_secs(30),
            request: Duration::from_secs(30),
        }
    }
}

/// Places for the account API's connections: `per_network` from one network at most, besides a
/// trusted proxy's, which count toward `total` alone.
#[derive(Clone)]
pub(crate) struct ApiPlaces {
    places: Places,
    proxies: Arc<[IpAddr]>,
}

impl ApiPlaces {
    pub fn new(total: usize, per_network: usize, proxies: &[IpAddr]) -> Self {
        Self {
            places: Places::new(total, per_network),
            proxies: proxies.iter().map(|p| p.to_canonical()).collect(),
        }
    }

    /// A place for a connection from `peer`, if one is free.
    pub fn take(&self, peer: IpAddr) -> Option<Place> {
        if self.proxies.contains(&peer.to_canonical()) {
            self.places.take_for_proxy()
        } else {
            self.places.take(peer)
        }
    }
}

/// An open connection, as axum gets it: the stream, the address it came in on, and its place
/// among the open connections, given back when it drops. A read or a write that waits fails,
/// which ends the connection, once no byte has come in or gone out for the idle time, or once a
/// request has taken longer than the request time: from its first byte to the server's answer,
/// as a request that trickles in a byte at a time never idles. Each answer starts both times
/// afresh.
pub struct Conn<S> {
    inner: S,
    local: SocketAddr,
    limits: ConnLimits,
    /// When a byte last came, or the server last answered, or the connection opened.
    active: Instant,
    /// When the request being read began: its first byte since the server last answered.
    request_from: Option<Instant>,
    deadline: Pin<Box<Sleep>>,
    _place: Place,
}

impl<S> Conn<S> {
    pub(crate) fn new(inner: S, local: SocketAddr, place: Place, limits: ConnLimits) -> Self {
        let active = Instant::now();
        Self {
            inner,
            local,
            limits,
            active,
            request_from: None,
            deadline: Box::pin(tokio::time::sleep_until(active + limits.idle)),
            _place: place,
        }
    }

    /// The address the connection came in on: this end's.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    pub fn get_ref(&self) -> &S {
        &self.inner
    }

    /// When a read or a write that waits fails: the idle time after the last activity, or the
    /// request time after the request's first byte, whichever comes first.
    fn due(&self) -> Instant {
        let idle = self.active + self.limits.idle;
        self.request_from
            .map_or(idle, |from| idle.min(from + self.limits.request))
    }

    /// The server wrote: the request it read is answered.
    fn answered(&mut self) {
        self.active = Instant::now();
        self.request_from = None;
        let due = self.due();
        self.deadline.as_mut().reset(due);
    }

    /// The error that ends a connection past its time.
    fn too_slow(&self) -> io::Error {
        let why = if self
            .request_from
            .is_some_and(|from| Instant::now() >= from + self.limits.request)
        {
            "the request took longer than the request time"
        } else {
            "no byte came in or went out for the idle time"
        };
        io::Error::new(io::ErrorKind::TimedOut, why)
    }

    /// A read or a write the stream keeps waiting: it fails once the deadline has passed. The
    /// deadline's timer wakes the connection's task then, as the stream may never do: a client
    /// that sends nothing, or reads nothing.
    fn wait<T>(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<T>> {
        match self.deadline.as_mut().poll(cx) {
            Poll::Ready(()) => Poll::Ready(Err(self.too_slow())),
            Poll::Pending => Poll::Pending,
        }
    }

    /// A write's outcome: bytes that went out answer the request read, and a write the stream
    /// keeps waiting waits within the deadline.
    fn wrote(
        &mut self,
        cx: &mut Context<'_>,
        wrote: Poll<io::Result<usize>>,
    ) -> Poll<io::Result<usize>> {
        match wrote {
            Poll::Ready(Ok(n)) if n > 0 => {
                self.answered();
                Poll::Ready(Ok(n))
            }
            Poll::Pending => self.wait(cx),
            ready => ready,
        }
    }
}

/// This end's address of `tcp`. The system always has it for a connected socket; otherwise the
/// unspecified address stands in, a gate launchers refuse.
pub(crate) fn local_of(tcp: &TcpStream) -> SocketAddr {
    tcp.local_addr()
        .unwrap_or(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
}

impl<S: AsyncRead + Unpin> AsyncRead for Conn<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) if buf.filled().len() > before => {
                let now = Instant::now();
                this.active = now;
                let from = *this.request_from.get_or_insert(now);
                // Bytes that keep coming past the request time don't buy it more.
                if now >= from + this.limits.request {
                    return Poll::Ready(Err(this.too_slow()));
                }
                let due = this.due();
                this.deadline.as_mut().reset(due);
                Poll::Ready(Ok(()))
            }
            // The end, or an error: nothing came.
            Poll::Ready(read) => Poll::Ready(read),
            Poll::Pending => this.wait(cx),
        }
    }
}

/// Writes wait within the same deadline as reads (Ruling HP10), as the gate's writes do within
/// theirs: a client that pipelines requests and never reads the answers backs the server's writes
/// up, and hyper, with requests still in its buffer, then reads no more, so a deadline polled by
/// reads alone would never come. Flushes and closes wait within it too: over TLS they write, the
/// records rustls holds and its closing alert.
impl<S: AsyncWrite + Unpin> AsyncWrite for Conn<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let wrote = Pin::new(&mut this.inner).poll_write(cx, buf);
        this.wrote(cx, wrote)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_flush(cx) {
            Poll::Pending => this.wait(cx),
            flushed => flushed,
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_shutdown(cx) {
            Poll::Pending => this.wait(cx),
            closed => closed,
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let wrote = Pin::new(&mut this.inner).poll_write_vectored(cx, bufs);
        this.wrote(cx, wrote)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

/// The account API's listener without TLS of its own: for loopback, or behind a proxy that holds
/// the certificate.
pub struct PlainListener {
    tcp: TcpListener,
    local: SocketAddr,
    places: ApiPlaces,
    limits: ConnLimits,
}

impl PlainListener {
    /// Listens on `tcp` under `limits`, `proxies` being `[server] trusted_proxies`.
    pub fn new(tcp: TcpListener, limits: ConnLimits, proxies: &[IpAddr]) -> io::Result<Self> {
        Ok(Self {
            local: tcp.local_addr()?,
            tcp,
            places: ApiPlaces::new(limits.open, limits.open_per_network, proxies),
            limits,
        })
    }
}

impl axum::serve::Listener for PlainListener {
    type Io = Conn<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            // axum's own accept: it waits out and logs the errors a TCP listener can meet.
            let (tcp, peer) = axum::serve::Listener::accept(&mut self.tcp).await;
            match self.places.take(peer.ip()) {
                Some(place) => {
                    let local = local_of(&tcp);
                    return (Conn::new(tcp, local, place, self.limits), peer);
                }
                None => tracing::debug!(%peer, "closed at once: no place free for it"),
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local)
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, SocketAddr};
    use std::time::{Duration, Instant};

    use axum::Router;
    use axum::extract::ConnectInfo;
    use axum::routing::{get, post};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::*;
    use crate::api::Addrs;

    /// A plain listener under `limits`, trusting `proxies`, whose `/` says `ok` when the
    /// connection's own address reached it, whose `/echo` reads a body and says its length, and
    /// whose `/big` answers with a mebibyte: its address.
    async fn serving(limits: ConnLimits, proxies: &[IpAddr]) -> SocketAddr {
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let listener = PlainListener::new(tcp, limits, proxies).unwrap();
        assert_eq!(axum::serve::Listener::local_addr(&listener).unwrap(), addr);
        let app = Router::new()
            .route(
                "/",
                get(move |ConnectInfo(a): ConnectInfo<Addrs>| async move {
                    if a.local == addr && a.peer != addr {
                        "ok"
                    } else {
                        "not this connection's addresses"
                    }
                }),
            )
            .route(
                "/echo",
                post(|body: String| async move { format!("got {}", body.len()) }),
            )
            .route("/big", get(|| async { vec![b'.'; 1 << 20] }));
        let service = app.into_make_service_with_connect_info::<Addrs>();
        tokio::spawn(axum::serve(listener, service).into_future());
        addr
    }

    /// Whether the server has closed `conn` within `within`: a read that ends, empty or reset.
    async fn closed_within(conn: &mut TcpStream, within: Duration) -> bool {
        let mut byte = [0; 1];
        matches!(
            tokio::time::timeout(within, conn.read(&mut byte)).await,
            Ok(Ok(0) | Err(_))
        )
    }

    #[test]
    fn the_limits_default_to_256_open_16_from_a_network_and_30_seconds_idle_or_for_a_request() {
        let d = ConnLimits::default();
        assert_eq!(
            (d.open, d.open_per_network, d.idle, d.request),
            (256, 16, Duration::from_secs(30), Duration::from_secs(30))
        );
    }

    /// Sends `head`, then `trickle` a byte every 100 ms, on a task of its own that keeps its half
    /// of the connection open after (dropping it would close it), and returns what the server
    /// answers before it closes the connection (within `within`), and when that was.
    async fn trickled(
        addr: SocketAddr,
        head: &'static [u8],
        trickle: &'static [u8],
        within: Duration,
    ) -> (String, Duration) {
        let started = Instant::now();
        let (mut rd, mut wr) = TcpStream::connect(addr).await.unwrap().into_split();
        tokio::spawn(async move {
            wr.write_all(head).await?;
            for byte in trickle {
                tokio::time::sleep(Duration::from_millis(100)).await;
                wr.write_all(&[*byte]).await?;
            }
            std::future::pending::<()>().await;
            std::io::Result::Ok(())
        });
        let mut answer = Vec::new();
        let _ = tokio::time::timeout(within, rd.read_to_end(&mut answer))
            .await
            .expect("closed within the time given");
        (
            String::from_utf8_lossy(&answer).into_owned(),
            started.elapsed(),
        )
    }

    #[tokio::test]
    async fn a_head_that_trickles_in_past_the_request_time_is_cut_off() {
        // Never idle for the idle time, but the request as a whole takes longer than it may.
        let limits = ConnLimits {
            idle: Duration::from_millis(300),
            request: Duration::from_millis(800),
            ..ConnLimits::default()
        };
        let addr = serving(limits, &[]).await;
        let (answer, took) = trickled(
            addr,
            b"GET / HTTP/1.1\r\n",
            b"Host: x\r\nX-Slow: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n\r\n",
            Duration::from_secs(10),
        )
        .await;
        assert!(!answer.contains("200 OK"), "{answer}");
        assert!(
            took >= limits.request && took < Duration::from_secs(3),
            "{took:?}"
        );
    }

    #[tokio::test]
    async fn a_body_that_trickles_in_past_the_request_time_is_cut_off() {
        let limits = ConnLimits {
            idle: Duration::from_millis(300),
            request: Duration::from_millis(800),
            ..ConnLimits::default()
        };
        let addr = serving(limits, &[]).await;
        let (answer, took) = trickled(
            addr,
            b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 40\r\n\r\n",
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Duration::from_secs(10),
        )
        .await;
        assert!(!answer.contains("got 40"), "{answer}");
        assert!(
            took >= limits.request && took < Duration::from_secs(3),
            "{took:?}"
        );
    }

    #[tokio::test]
    async fn a_body_that_stops_is_cut_off_after_the_idle_time() {
        let limits = ConnLimits {
            idle: Duration::from_millis(300),
            ..ConnLimits::default()
        };
        let addr = serving(limits, &[]).await;
        let (answer, took) = trickled(
            addr,
            b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 40\r\n\r\naaaa",
            b"",
            Duration::from_secs(3),
        )
        .await;
        assert!(!answer.contains("got"), "{answer}");
        assert!(took >= limits.idle, "{took:?}");
    }

    /// Whether a new connection to `addr` is served now: answered `200` within a second.
    async fn served_now(addr: SocketAddr) -> bool {
        let ask = async {
            let mut conn = TcpStream::connect(addr).await?;
            conn.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
                .await?;
            let mut answer = String::new();
            conn.read_to_string(&mut answer).await?;
            std::io::Result::Ok(answer)
        };
        matches!(
            tokio::time::timeout(Duration::from_secs(1), ask).await,
            Ok(Ok(answer)) if answer.starts_with("HTTP/1.1 200")
        )
    }

    #[tokio::test]
    async fn a_client_that_never_reads_its_answers_gives_its_place_back() {
        // Ruling HP10, the review's probe: 3,000 pipelined requests whose answers the client never
        // reads. Once the answers outgrow what the sockets' buffers take (2.6 MiB on the Linux
        // machine this was written on, so a mebibyte each), the server's writes wait, and with
        // requests still in hyper's buffer it reads no more: only the writes' own deadline ends
        // the connection, and gives its network's one place back.
        let limits = ConnLimits {
            open_per_network: 1,
            idle: Duration::from_millis(300),
            request: Duration::from_millis(300),
            ..ConnLimits::default()
        };
        let addr = serving(limits, &[]).await;
        let (unread, mut wr) = TcpStream::connect(addr).await.unwrap().into_split();
        let pipelined = b"GET /big HTTP/1.1\r\nHost: x\r\n\r\n".repeat(3000);
        let pipeliner = tokio::spawn(async move {
            let _ = wr.write_all(&pipelined).await;
            // Its half stays open: dropping it would close the connection.
            std::future::pending::<()>().await;
        });
        let started = Instant::now();
        assert!(!served_now(addr).await, "its place is held at first");
        while !served_now(addr).await {
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "the place is still held after {:?}",
                started.elapsed()
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        pipeliner.abort();
        drop(unread);
    }

    /// A stream that never takes a byte or gives one, and never wakes a task that waits on it: a
    /// client that sends nothing, and whose window stays shut.
    struct Stuck;

    impl AsyncRead for Stuck {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }

    impl AsyncWrite for Stuck {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Pending
        }

        fn poll_write_vectored(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &[io::IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            Poll::Pending
        }

        fn is_write_vectored(&self) -> bool {
            true
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }

        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }

    #[tokio::test]
    async fn every_wait_on_a_stuck_connection_ends_at_the_idle_time() {
        // Ruling HP10: however the server writes, a write that waits fails once the deadline has
        // passed, as a read does. Over TLS a flush or a close waits on the socket as a write does.
        let limits = ConnLimits {
            idle: Duration::from_millis(200),
            ..ConnLimits::default()
        };
        let started = Instant::now();
        let places = ApiPlaces::new(5, 5, &[]);
        let stuck = || {
            let place = places.take(IpAddr::from([10, 0, 0, 1])).unwrap();
            Conn::new(
                Stuck,
                SocketAddr::from(([127, 0, 0, 1], 7801)),
                place,
                limits,
            )
        };
        let (mut reading, mut writing, mut vectored) = (stuck(), stuck(), stuck());
        let (mut flushing, mut closing) = (stuck(), stuck());
        let (mut byte, answer) = ([0; 1], [io::IoSlice::new(b"an answer")]);
        let ends = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(
                reading.read(&mut byte),
                writing.write(b"an answer"),
                vectored.write_vectored(&answer),
                flushing.flush(),
                closing.shutdown(),
            )
        })
        .await
        .expect("every wait ends within its time");
        let took = started.elapsed();
        assert!(took >= limits.idle, "not before the idle time: {took:?}");
        let (read, wrote, wrote_vectored, flushed, closed) = ends;
        let kinds = [
            read.map(drop),
            wrote.map(drop),
            wrote_vectored.map(drop),
            flushed,
            closed,
        ]
        .map(|end| end.map_err(|e| e.kind()));
        assert_eq!(kinds, [Err(io::ErrorKind::TimedOut); 5]);
    }

    #[tokio::test]
    async fn requests_answered_in_time_keep_their_connection() {
        // Each answer starts the request time afresh: a connection that asks quickly, again and
        // again, for longer in all than the request time, is never cut off.
        let limits = ConnLimits {
            idle: Duration::from_millis(500),
            request: Duration::from_millis(500),
            ..ConnLimits::default()
        };
        let addr = serving(limits, &[]).await;
        let mut conn = TcpStream::connect(addr).await.unwrap();
        for _ in 0..8 {
            tokio::time::sleep(Duration::from_millis(150)).await;
            conn.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
                .await
                .unwrap();
            let mut answer = [0; 1024];
            let n = tokio::time::timeout(Duration::from_secs(2), conn.read(&mut answer))
                .await
                .expect("an answer")
                .unwrap();
            let answer = String::from_utf8_lossy(&answer[..n]);
            assert!(
                answer.starts_with("HTTP/1.1 200") && answer.ends_with("ok"),
                "{answer}"
            );
        }
    }

    #[tokio::test]
    async fn a_connection_that_sends_nothing_is_closed_after_the_idle_time() {
        let limits = ConnLimits {
            idle: Duration::from_millis(300),
            ..ConnLimits::default()
        };
        let addr = serving(limits, &[]).await;
        let started = Instant::now();
        let mut conn = TcpStream::connect(addr).await.unwrap();
        assert!(closed_within(&mut conn, Duration::from_secs(3)).await);
        let took = started.elapsed();
        assert!(took >= limits.idle, "not before the idle time: {took:?}");
    }

    #[tokio::test]
    async fn each_byte_that_comes_puts_the_idle_time_off() {
        // A request trickling in a byte at a time, slower in all than the idle time but never
        // idle that long, and within the request time, is answered.
        let limits = ConnLimits {
            idle: Duration::from_millis(300),
            request: Duration::from_secs(20),
            ..ConnLimits::default()
        };
        let addr = serving(limits, &[]).await;
        let mut conn = TcpStream::connect(addr).await.unwrap();
        conn.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
        for byte in b"Host: x\r\nConnection: close\r\n" {
            tokio::time::sleep(Duration::from_millis(100)).await;
            conn.write_all(&[*byte]).await.unwrap();
        }
        conn.write_all(b"\r\n").await.unwrap();
        let mut answer = String::new();
        conn.read_to_string(&mut answer).await.unwrap();
        assert!(
            answer.starts_with("HTTP/1.1 200") && answer.ends_with("ok"),
            "{answer}"
        );
    }

    #[test]
    fn a_trusted_proxy_counts_toward_the_total_alone() {
        // A proxy's connections carry many players' requests: its network's share doesn't bind
        // it, the total does.
        let places = ApiPlaces::new(3, 1, &["::ffff:127.0.0.1".parse().unwrap()]);
        let player = places.take("10.0.0.1".parse().unwrap()).unwrap();
        assert!(
            places.take("10.0.0.1".parse().unwrap()).is_none(),
            "one from a network"
        );
        let proxy: IpAddr = "127.0.0.1".parse().unwrap();
        let held = [places.take(proxy).unwrap(), places.take(proxy).unwrap()];
        assert!(places.take(proxy).is_none(), "the total binds a proxy too");
        drop(player);
        assert!(
            places.take(proxy).is_some(),
            "a place given back is free again"
        );
        drop(held);
    }

    /// Places by network, from a second loopback address: Linux routes all of 127.0.0.0/8 to
    /// loopback, where macOS has 127.0.0.1 alone unless told otherwise.
    #[cfg(target_os = "linux")]
    mod other_networks {
        use tokio::net::TcpSocket;

        use super::*;

        async fn from(source: &str, addr: SocketAddr) -> TcpStream {
            let sock = TcpSocket::new_v4().unwrap();
            sock.bind(format!("{source}:0").parse().unwrap()).unwrap();
            sock.connect(addr).await.unwrap()
        }

        #[tokio::test]
        async fn one_network_past_its_places_is_closed_at_once_and_another_gets_in() {
            let limits = ConnLimits {
                open_per_network: 3,
                ..ConnLimits::default()
            };
            let addr = serving(limits, &[]).await;
            let mut held = Vec::new();
            for _ in 0..3 {
                held.push(from("127.0.0.2", addr).await);
            }
            let mut fourth = from("127.0.0.2", addr).await;
            assert!(closed_within(&mut fourth, Duration::from_secs(2)).await);
            for conn in &mut held {
                assert!(!closed_within(conn, Duration::from_millis(50)).await);
            }
            let mut other = from("127.0.0.1", addr).await;
            other
                .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            let mut answer = String::new();
            other.read_to_string(&mut answer).await.unwrap();
            assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        }

        #[tokio::test]
        async fn a_trusted_proxy_holds_more_than_one_networks_places() {
            let limits = ConnLimits {
                open_per_network: 2,
                ..ConnLimits::default()
            };
            let addr = serving(limits, &["127.0.0.2".parse().unwrap()]).await;
            let mut proxied = Vec::new();
            for _ in 0..5 {
                proxied.push(from("127.0.0.2", addr).await);
            }
            for conn in &mut proxied {
                assert!(!closed_within(conn, Duration::from_millis(50)).await);
            }
            let mut held = [from("127.0.0.3", addr).await, from("127.0.0.3", addr).await];
            let mut third = from("127.0.0.3", addr).await;
            assert!(closed_within(&mut third, Duration::from_secs(2)).await);
            for conn in &mut held {
                assert!(!closed_within(conn, Duration::from_millis(50)).await);
            }
        }
    }
}
