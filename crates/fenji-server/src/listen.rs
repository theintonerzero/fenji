//! The account API's connections (M4 hosting spec §3.10; Ruling HP9). Both of its listeners, plain
//! and TLS, accept every connection, then give it one of the open connections' places, counted
//! by the peer's network as the gate counts its own (M3 spec D17), or close it at once; and they
//! hand axum each one in a [`Conn`], which keeps its place until it closes and ends it once it has
//! sent nothing for the idle time. axum gives hyper no timer, so hyper's own limit on reading a
//! request's head never applies: without these, connections that send nothing would stay open
//! until the process ran out of file descriptors, for the gate as for the API.
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
    /// No byte from a connection for this long closes it, whatever it was waiting for.
    pub idle: Duration,
}

impl Default for ConnLimits {
    fn default() -> Self {
        Self {
            open: 256,
            open_per_network: 16,
            idle: Duration::from_secs(30),
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
/// among the open connections, given back when it drops. A read that waits longer than the idle
/// time fails, which ends the connection; each byte that comes puts the time off.
pub struct Conn<S> {
    inner: S,
    local: SocketAddr,
    idle: Duration,
    deadline: Pin<Box<Sleep>>,
    _place: Place,
}

impl<S> Conn<S> {
    pub(crate) fn new(inner: S, local: SocketAddr, place: Place, idle: Duration) -> Self {
        Self {
            inner,
            local,
            idle,
            deadline: Box::pin(tokio::time::sleep(idle)),
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
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(read) => {
                this.deadline.as_mut().reset(Instant::now() + this.idle);
                Poll::Ready(read)
            }
            Poll::Pending => match this.deadline.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "nothing came for the idle time",
                ))),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Conn<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
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
    idle: Duration,
}

impl PlainListener {
    /// Listens on `tcp` under `limits`, `proxies` being `[server] trusted_proxies`.
    pub fn new(tcp: TcpListener, limits: ConnLimits, proxies: &[IpAddr]) -> io::Result<Self> {
        Ok(Self {
            local: tcp.local_addr()?,
            tcp,
            places: ApiPlaces::new(limits.open, limits.open_per_network, proxies),
            idle: limits.idle,
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
                    return (Conn::new(tcp, local, place, self.idle), peer);
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
    use axum::routing::get;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::*;
    use crate::api::Addrs;

    /// A plain listener under `limits`, trusting `proxies`, whose `/` says `ok` when the
    /// connection's own address reached it: its address.
    async fn serving(limits: ConnLimits, proxies: &[IpAddr]) -> SocketAddr {
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let listener = PlainListener::new(tcp, limits, proxies).unwrap();
        assert_eq!(axum::serve::Listener::local_addr(&listener).unwrap(), addr);
        let app = Router::new().route(
            "/",
            get(move |ConnectInfo(a): ConnectInfo<Addrs>| async move {
                if a.local == addr && a.peer != addr {
                    "ok"
                } else {
                    "not this connection's addresses"
                }
            }),
        );
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
    fn the_limits_default_to_256_open_16_from_a_network_and_30_seconds_idle() {
        let d = ConnLimits::default();
        assert_eq!(
            (d.open, d.open_per_network, d.idle),
            (256, 16, Duration::from_secs(30))
        );
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
        // idle that long, is answered.
        let limits = ConnLimits {
            idle: Duration::from_millis(300),
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
