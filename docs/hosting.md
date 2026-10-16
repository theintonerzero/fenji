# Hosting a Fenji server

This guide is for hosts: anyone who runs a Fenji server for friends they invite. Hosting is meant to be as easy as hosting a Minecraft server. You unpack the server, run its start script, type `invite` in its console, and send the link to a friend. Friends on your network can join at once, and friends elsewhere join through an open port or a VPN.

Players need only the desktop launcher and the link you send them: the [README](../README.md#playing) says how they play.

1. [Before you start](#1-before-you-start)
2. [On your own computer](#2-on-your-own-computer)
3. [Letting friends in](#3-letting-friends-in)
4. [On a VPS: the reference deployment](#4-on-a-vps-the-reference-deployment)
5. [Updating](#5-updating)
6. [Backups and restoring](#6-backups-and-restoring)
7. [Troubleshooting](#7-troubleshooting)

## 1. Before you start

- **Every player needs their own copy of the game,** you included: Swords of Legends Online on Steam (app 1418100, which you can download only if it's already in your library, since it was delisted), or from Gameforge's launcher back when Gameforge distributed it. Fenji never distributes the game or any of its files. The server itself doesn't need the game's files yet.
- **Your friends play on Linux, for now.** In v0.5 the desktop launcher starts the game on Linux only. On Windows and macOS it does the rest, such as making an account with your link, but can't start the game yet ([README](../README.md#playing)).
- **Trust goes both ways.** Your players trust you: the game client obeys the server it's connected to, and a server someone has changed could make it write files or run code on their computers ([PRIVACY.md](../PRIVACY.md#trusting-a-servers-host)). You hold their data: their accounts, with password hashes, their characters, and logs that show the addresses they connect from. [PRIVACY.md](../PRIVACY.md) says what a server keeps. As its host you're its operator, and that data is yours to look after. When a player asks you to delete their account, the server's `account delete` command does it (section 2).
- **The law.** The game's terms likely forbid running it against unofficial servers, and its publishers do pursue private servers, so hosting is at your own risk ([README](../README.md#legal)). Fenji is for private, invite-only, non-commercial servers, and the project doesn't run, list or vouch for any.
- **A computer that stays on while your friends play:** your own, with Windows, macOS on Apple silicon or Linux (section 2), or a rented Linux server, a VPS (section 4).

## 2. On your own computer

### Download and start

1. **Download** the archive for your system from the [latest release](https://github.com/theintonerzero/fenji/releases/latest). Releases from v0.5.0 have them.

   | System | Archive | Start script |
   |---|---|---|
   | Windows | `fenji-server-<version>-x86_64-pc-windows-msvc.zip` | `start-server.bat` |
   | macOS (Apple silicon) | `fenji-server-<version>-aarch64-apple-darwin.tar.gz` | `start-server.command` |
   | Linux (x86_64) | `fenji-server-<version>-x86_64-unknown-linux-musl.tar.gz` | `start-server.sh` |
   | Linux (ARM, 64-bit) | `fenji-server-<version>-aarch64-unknown-linux-musl.tar.gz` | `start-server.sh` |

2. **Unpack it** in your home folder, or anywhere else on your computer's own disk. Not on a USB stick or another FAT or exFAT drive: those drives can't keep a folder private, and the server refuses to keep its key in a folder that other users can open. It unpacks into a folder named for its version, such as `fenji-server-0.5.0-x86_64-unknown-linux-musl`. Rename the folder with the start script in it to `fenji-server`: it's your server's folder from now on, and each update goes into it (section 5).
3. **Start it** with the start script in that folder:
   - **Windows:** double-click `start-server.bat`. The first time, SmartScreen says "Windows protected your PC", because the server isn't signed by Microsoft: choose More info, then Run anyway. When Windows asks whether to let `fenji-server.exe` through its firewall, allow it on private networks.
   - **macOS:** double-click `start-server.command`, which opens in Terminal. The first time, macOS says Apple can't check it, because the server isn't signed by Apple: close the message, open System Settings, Privacy & Security, and choose Open Anyway beside its name. macOS may then ask the same about `fenji-server` itself, and you allow it the same way. If macOS asks whether `fenji-server` may accept incoming network connections, allow it.
   - **Linux:** open a terminal in that folder (in your file manager, right-click the folder and choose Open in Terminal), and run:

     ```sh
     ./start-server.sh
     ```

The first start writes the server's settings, `fenji.toml`, and its `data` folder beside the start script, and the console opens: the server's log above, and a command line below. Its first lines say what happened:

```
09:41:07 INFO  First run: wrote fenji.toml and data/ in /home/wen/fenji-server.
09:41:07 INFO  made the server's own key in data/tls: invite links carry its pin, so keep the folder with your backups pin=8pS1fXyQe0VnJ4cLr6TgWm2HbKd7uZaN3oYiE5qRtUw
09:41:07 INFO  Fenji is running gate=0.0.0.0:7800 api=0.0.0.0:7801
09:41:07 INFO  server link (enp3s0): https://192.0.2.10:7801/#pin=8pS1fXyQe0VnJ4cLr6TgWm2HbKd7uZaN3oYiE5qRtUw (type link for the others)
09:41:07 INFO  Friends on this network can join now: type `invite` for a link.
09:41:07 INFO  To let in friends over the internet, open ports 7800 and 7801 on your router, or use a VPN: see https://github.com/theintonerzero/fenji/blob/v0.5.0/docs/hosting.md.
```

The times are in UTC, as the console's title bar says, so they may differ from your clock.

The server is ready for your network as it starts: it listens on all of your computer's network connections, on ports 7800 (the game) and 7801 (the account API, where launchers sign players in), and the account API speaks TLS with the server's own key.

### Invite a friend

Type `invite` and press Enter:

```
link (enp3s0): https://192.0.2.10:7801/#invite=K4QW-7TPM-2XHN-9CRB&pin=8pS1fXyQe0VnJ4cLr6TgWm2HbKd7uZaN3oYiE5qRtUw
server link (enp3s0): https://192.0.2.10:7801/#pin=8pS1fXyQe0VnJ4cLr6TgWm2HbKd7uZaN3oYiE5qRtUw
link (this computer): https://127.0.0.1:7801/#invite=K4QW-7TPM-2XHN-9CRB&pin=8pS1fXyQe0VnJ4cLr6TgWm2HbKd7uZaN3oYiE5qRtUw
server link (this computer): https://127.0.0.1:7801/#pin=8pS1fXyQe0VnJ4cLr6TgWm2HbKd7uZaN3oYiE5qRtUw
code: K4QW-7TPM-2XHN-9CRB
friends over the internet: open ports 7800 and 7801 on your router, or use a VPN: see https://github.com/theintonerzero/fenji/blob/v0.5.0/docs/hosting.md
```

There's an invite link for each of your computer's addresses, labelled with its network connection, and one for this computer alone. Send your friend the invite link for the network they share with you, by any channel you trust: it makes one account, once. They paste it into the desktop launcher, which makes their account and keeps your server in its list. A link is for the launcher, not a web browser: a browser warns about the server's own certificate, and finds no page there. Beside each invite link is your server's own link, its `server link`, without the invite: that one is for players who already have an account (below).

**Copying a link.** Select it with the mouse, from its first letter to its last, then copy it: Ctrl-Shift-C on Linux, Cmd-C on macOS, or Ctrl-C on Windows. In a Linux terminal, Ctrl-C stops the server instead, cleanly, so use Ctrl-Shift-C there. A link is longer than many windows are wide, so it may show on two rows or more: the console starts its later rows at the window's left edge, so that the copy holds only line breaks, which the launchers leave out. For each link on one row, make the window wider until it fits, or run `./fenji-server invite create` in another terminal, in the server's folder (below), which prints each link whole, as `--plain` does.

- `invite Mai` labels the invite, so that `invites` shows whom it was for.
- An invite made at the console stays valid until it's used or revoked. The `invite create` command makes one that runs out (below).
- The `pin` in the link identifies your server's key, and the launcher checks that the server it reaches has that key. It stays the same as long as you keep the `data/tls` folder (section 6).
- To play on your own server, invite yourself with the `this computer` link, and make your account an op with `op <your account>`, so you can use the admins' commands in the game's chat.

**A player who already has an account** and adds your server to a launcher on another computer needs your server's link without an invite, and so does every player after your server's key changes (section 7). Type `link`: it shows your server's link for each of your computer's addresses, as `invite` does, and makes no invite.

```
server link (enp3s0): https://192.0.2.10:7801/#pin=8pS1fXyQe0VnJ4cLr6TgWm2HbKd7uZaN3oYiE5qRtUw
server link (this computer): https://127.0.0.1:7801/#pin=8pS1fXyQe0VnJ4cLr6TgWm2HbKd7uZaN3oYiE5qRtUw
friends over the internet: open ports 7800 and 7801 on your router, or use a VPN: see https://github.com/theintonerzero/fenji/blob/v0.5.0/docs/hosting.md
```

The player adds that link as a server, and signs in. The console also shows one in its log as the server starts; copy links from `link` or `invite`, though, as a log line's later rows stay under its text, and a copy of one would hold their spaces. A server behind Caddy, with a domain name (section 4), has no pin: its link is its address alone, such as `https://play.example.net`, and `link` shows that.

### The console

The console works like a Minecraft server's. The log scrolls above, the command line is at the bottom, and a status line at the top shows the server's state, its name, its version, how long it has run, how many players are online of how many it takes, and its ports.

- Type a command and press Enter. While you type its first word, or when you press Tab, a list shows the commands that match: ↑ and ↓ choose, Tab completes, Enter runs and Esc closes it. With the list closed, ↑ and ↓ go back through the commands you typed.
- PgUp and PgDn scroll back through the last 5,000 lines of the log. The mouse wheel doesn't: the console turns off the terminal's habit of turning it into ↑ and ↓, which would bring back your earlier commands instead.
- The log's times are in UTC, as the title bar says (`times in UTC`).
- `stop`, or Ctrl-C, stops the server cleanly: it lets every player go, saves where each one stood, and says `Fenji has stopped` with where its log is. Closing the window, or losing the SSH connection the server runs in, stops it cleanly too, though there's no window left to say so. A server started with SIGHUP ignored, as `trap '' HUP` leaves it, runs on instead, without a console, and its log says so: `./fenji-server console` attaches to it, and `./fenji-server console --run stop` stops it. On Windows, Windows ends the server a few seconds after its window closes, and the server saves where everyone stood first.
- The window needs to be at least 60 columns by 15 rows. A smaller one shows a note asking for a bigger window, and the server keeps running.
- The colours are exact in a terminal with 24-bit colour, and close elsewhere. With `NO_COLOR` set there are none. Any monospaced font works; Geist Mono, under the SIL Open Font License, suits the console's design.
- This full-screen console also writes the log to files in `data/logs`, one for each day the server runs (`fenji.2026-10-16.log`, by the day in UTC), and keeps those of the last 14 days it ran. Attach the day's file to a bug report.
- If the console itself fails, it gives the terminal back and says so. The server keeps running, and Ctrl-C, or closing the terminal, stops it.

**Plain lines,** for a screen reader or a simple terminal: start the server with `--plain`, and the console reads commands as lines and prints their answers, with the log as plain lines too.

```sh
./start-server.sh --plain
```

On Windows, open a Command Prompt in the server's folder and run `start-server.bat --plain`. With `--plain`, the log goes to the terminal only, and no file is written. To keep it, send it to a file: the screen then shows only the commands' answers, and the file gets the log, an error the server stops with included.

```sh
./start-server.sh --plain 2>> fenji.log
```

**Over SSH,** on a computer at home or a server without the service of section 4, a server started in a bare SSH session stops cleanly when the connection goes. To keep it running, start it inside `tmux` or `screen`:

```sh
tmux new -s fenji ./start-server.sh
```

Press Ctrl-B, then D, to leave it running, and `tmux attach -t fenji` brings it back. Or start it with `nohup`, which keeps it running after the connection goes, with its log in a file and no console of its own; attach to it from another terminal (below):

```sh
nohup ./start-server.sh --plain < /dev/null >> fenji.log 2>&1 &
```

**One server to a data folder.** A running server holds `server.lock` in its data folder, on every system, so that two servers never use one data folder: a second one refuses to start (section 7). The system lets go of the lock however the server ends, a crash included.

**Attaching from another terminal** (Linux and macOS). A running server has a console socket, `console.sock`, in its data folder, which another terminal can attach to. In the server's folder, run:

```sh
./fenji-server console
```

It shows the same console. Ctrl-C, `exit` or `quit` leave it, and the server runs on; `stop` stops the server. Four consoles can be attached at once. Attach as the user the server runs as: the socket is that user's alone. `--plain` works here too.

`--run` runs one command for a script, prints its answer, and exits with 0 if the command did what it asked, or 1 if it didn't or no server is running:

```sh
./fenji-server console --run online
```

`--run stop` returns once the server has stopped, so that a script can start it again at once. It waits a minute at most, and exits with 1 if the server hasn't stopped by then, saying so.

- A data folder whose path is too long for a socket (107 bytes on Linux and 103 on macOS, with `console.sock`) gets none: the server still starts, says why, and runs without attaching. The start scripts use a short relative path, so this happens only to a server started with a `--config` deep in the file system.
- If an attached console stops reading, such as over a frozen SSH connection, a stopping server waits up to 5 seconds for it before it exits.
- Windows has no console socket: there, the console is in the server's window.

### The console's commands

Commands work in any case. `help` lists them, and `help <command>` shows one's usage. `tp` and `kick` take characters' names; the account commands take account names.

| Command | What it does |
|---|---|
| `invite [<label>]` | Makes an invite: a link for each of your computer's addresses, your server's own link beside each, and its code. The label, at most 64 characters, says whom it's for. |
| `link` | Shows your server's own link for each of your computer's addresses, without making an invite: for players who already have an account. |
| `invites` | Lists the invites, each with its id, its label, when it was made and whether it was used, never their codes. |
| `revoke <id>` | Revokes an invite that nobody has used. |
| `accounts` | Lists the accounts, with when each was made and last signed in, and how many characters it has. |
| `online`, or `list` | Who is connected: the account, the character it plays or `lobby`, the scene, and since when. Never an address. |
| `announce <text>`, or `say <text>` | Shows the text to everyone in the game, on the marquee: at most 140 characters, without backslashes or words in square brackets, which the game would read as links or sounds. |
| `kick <name>` | Lets the player go, with the kick reason. They can come back: `ban` keeps an account out. |
| `tp <name> <name>` | Takes the first player to the second, in the same scene. |
| `motd` | Shows the message of the day. |
| `motd <text>` | Sets the message of the day, at once: plain text of at most 200 characters, which launchers show players as your notice, and which each player gets once in the game's chat when their character has loaded into the world. It's kept in the database, over `motd` in `fenji.toml`. |
| `motd off` | Drops the message the console set, so that `motd` in `fenji.toml` is the message of the day again: none, unless you wrote one there. |
| `maintenance` | Shows whether maintenance is on, with its message and times. |
| `maintenance on [<duration>] [<message>]` | Turns maintenance on: only admins can sign in or start the game, and nobody can use an invite, until it's off. Players in the game stay until you kick them or stop the server. The duration (`30m`, `2h` or `1d`) is when you expect to be back, which players are shown; it doesn't end maintenance by itself. The message, at most 200 characters, is what players read; without one it's `Back soon`, even if maintenance was on with another, so give it again to keep it. Everyone in the game reads `Maintenance: <message>`. Maintenance stays on when the server restarts. |
| `maintenance off` | Turns maintenance off, and everyone in the game reads `Maintenance is over.` |
| `op <account>` | Makes the account an admin, at once, even for a player in the game. Admins can use `!tp`, `!kick`, `!announce` and `!invite` in the game's chat, and sign in during maintenance. Ops are kept in the database, beside `admins` in `fenji.toml`. |
| `deop <account>` | Takes the account's op away, at once. An account that `admins` in `fenji.toml` names stays an admin until you take it out of the file and restart the server. |
| `ops` | Lists the admins, and why each is one: an op, `admins` in `fenji.toml`, or both. |
| `ban <account> [<reason>]` | Bans the account: it can't sign in or start the game, and its player reads the reason, plain text of at most 200 characters. If it's connected, it's let go at once, and the lobby won't let it into the world. |
| `pardon <account>` | Lifts the account's ban. |
| `bans` | Lists the bans, with their reasons and since when. |
| `backup` | Copies the database into the backups folder (section 6), keeping the last 14 copies. It's safe while players play. |
| `stop` | Stops the server cleanly, as Ctrl-C does in its own window. From an attached console too, it stops the server, not just the console. |
| `help [<command>]` | Lists the commands, or shows one's usage. |

Signed-in players' launchers list the characters in the world, those still loading in among them. The console logs every command it runs.

### Commands outside the console

The server's program takes commands of its own too, for scripts and for times the server isn't running; `--help` lists them. They're safe while the server runs. Run them from the server's folder, as the user the server runs as:
- on Linux and macOS, as `./fenji-server <command>`;
- on Windows, in a Command Prompt, as `fenji-server.exe <command>`;
- on the reference VPS (section 4), as `sudo -u fenji fenji-server --config /etc/fenji/fenji.toml <command>`. Run as root, they refuse, and say whom to run them as: a file root made in the data folder would be root's, and the service couldn't use it. The one exception is `init`, which writes the settings into `/etc/fenji`, where only root may write, and touches no data (section 4).

The server and these commands refuse a data folder that another user owns, or that every user on the computer may write in, before they write anything there (section 7).

| Command | What it does |
|---|---|
| `invite create [--label <label>] [--expires <time>]` | Makes an invite, as the console's `invite` does; with `--expires` (`30m`, `12h` or `7d`), one that runs out unused. |
| `invite list`, `invite revoke <id>` | As the console's `invites` and `revoke`. |
| `account list` | As the console's `accounts`. |
| `account delete <account>` | Deletes the account with its characters, as a player may ask you to ([PRIVACY.md](../PRIVACY.md#deleting-your-data)). |
| `backup [--keep <n>]` | Makes a backup now, keeping the last 14, or `n` (section 6). |
| `console [--run <command>]` | Attaches to the running server's console (above). |
| `init` | Writes a settings file to edit (section 4). |

On Linux, for example, an invite for a week:

```sh
./fenji-server invite create --label Mai --expires 7d
```

### Settings

`fenji.toml` holds the server's settings, and `fenji.example.toml` beside it describes every one. A change takes effect when the server restarts. The ones a host changes most:
- `name`: your server's name, which the game and launchers show (at most 32 characters, and no colons);
- `host`: your name, which launchers show as "hosted by";
- `motd`: the message of the day, unless the console sets one;
- `max_players`: how many players can be online at once (20 unless you change it);
- `admins`: accounts that are admins whatever the console's `op` and `deop` say.

The server keeps a file open for each connection. The start scripts raise the limit on open files to 4,096, as far as the system allows (macOS gives a program 256 by default), and the reference deployment's service sets it (section 4). If you start `fenji-server` some other way, raise it too, in the same shell first:

```sh
ulimit -n 4096
```

## 3. Letting friends in

### On the same network

Friends on your network, at home or on the same Wi-Fi, can join as the server starts: send them the link for that network (section 2). If your computer has a firewall, it must let in ports 7800 and 7801. Windows and macOS ask once, and you allow it. On Linux, desktop systems usually let these ports in already; if yours runs a firewall that doesn't, open them with firewalld:

```sh
sudo firewall-cmd --permanent --add-port=7800/tcp --add-port=7801/tcp && sudo firewall-cmd --reload
```

or with ufw:

```sh
sudo ufw allow 7800/tcp && sudo ufw allow 7801/tcp
```

On Windows, your network must be a private one, in Settings, Network & internet, for the firewall's choice to apply.

### Over the internet, with open ports

This is the usual way, and your friends install nothing more. First, what it exposes: the game connection, on port 7800, is readable and authenticates nobody. Anyone on the network path between a player and your server can read what crosses it, and could pose as either side. That's how the game client was built ([`transport.md`](protocol/transport.md) 3.9, [PRIVACY.md](../PRIVACY.md#the-game-connection)). No password crosses it: passwords go only to the account API, on port 7801, which speaks TLS, and the game connection carries a ticket that works once, for one account, within a minute. But someone on that path could take over a player's session. If that matters to your group, use a VPN (below). Anyone on the internet can reach both ports, as with any server: the API makes accounts only from your invites, the game lets in only players with a ticket from the API, and both limit how many connections each address may open.

1. **Forward the ports.** In your router's settings, find port forwarding (also called virtual servers or NAT rules), and forward TCP ports 7800 and 7801 to your computer's address on your network, with the same numbers outside and in. That address is the one in your network's invite link. Give your computer a fixed address on your network (a DHCP reservation, in the router), so that the forwarding keeps pointing at it.
2. **Open the firewall** on your computer, as above.
3. **Find your public address.** Your router's status page shows it, as its internet or WAN address.
4. **Make the link.** Take your network's invite link and put your public address in place of your computer's: `https://192.0.2.10:7801/#invite=…&pin=…` becomes `https://198.51.100.7:7801/#invite=…&pin=…`. Your friend's game then connects to the address in their link, and friends on your network keep using yours.

Your public address may change from time to time; then players can't reach your server until they add it again with the new address. A domain name that follows your address (dynamic DNS, which many routers can keep up to date) avoids that: use the name in place of the address. A router whose internet address starts with 100.64 to 100.127, or a provider that shares one address among its customers (carrier-grade NAT), can't forward ports at all: use a VPN that finds its own way through, such as Tailscale or ZeroTier (below).

Testing your public address from inside your own network may fail even when it works for friends outside, as many routers don't loop it back. Ask a friend to add your server in their launcher, which shows whether it's online.

### Over a VPN

A VPN puts your friends on a private network with you, over the internet: Tailscale, ZeroTier or WireGuard, for example. It's the private option, since the VPN encrypts the game connection, and there are no ports to forward. Tailscale and ZeroTier find their own way through NAT, carrier-grade NAT included. Plain WireGuard needs one computer that the others can reach, such as one with a port forwarded, or a VPS. Each friend joins your VPN, as the VPN's own guide says. Then `invite` gives a link for the VPN's network connection too (such as `tailscale0` or `wg0`): send them that one.

## 4. On a VPS: the reference deployment

A VPS keeps your server running when your own computer is off. This is Fenji's reference setup, for a Linux VPS with systemd, such as Debian 13 or Ubuntu 24.04. The server runs as a service of its own user, starts with the machine, comes back after a crash, and backs itself up each night. Its program is `/usr/local/bin/fenji-server`, its settings are in `/etc/fenji/fenji.toml`, and its data folder is `/var/lib/fenji`. The commands are for Debian and Ubuntu, from an SSH session.

They use `sudo` and `curl`, which a minimal system may lack. Signed in as root, install them first; with `sudo` already there, put `sudo` before each `apt`:

```sh
apt update && apt install -y sudo curl ca-certificates
```

Some steps use `VERSION`, which the first one sets in your shell. In a new SSH session, set it again before a step that uses it.

### Install the server

Set `VERSION` to the latest release's number, without its `v`, and download that release's archive for your server's processor, with its checksums:

```sh
VERSION=0.5.0
TARGET=$(uname -m)-unknown-linux-musl
curl -fLO https://github.com/theintonerzero/fenji/releases/download/v$VERSION/fenji-server-$VERSION-$TARGET.tar.gz
curl -fLO https://github.com/theintonerzero/fenji/releases/download/v$VERSION/SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
```

The last command should print the archive's name and `OK`. Then install the program, and make the user it runs as, `fenji`, which owns the server's data and nothing else:

```sh
tar -xzf fenji-server-$VERSION-$TARGET.tar.gz
sudo install -m 755 fenji-server-$VERSION-$TARGET/fenji-server /usr/local/bin/fenji-server
sudo useradd --system --user-group --home-dir /var/lib/fenji --shell /usr/sbin/nologin fenji
```

### Its settings

Write the settings, and open them in an editor:

```sh
sudo fenji-server --config /etc/fenji/fenji.toml init
sudo nano /etc/fenji/fenji.toml
```

A minimal system may have no nano. Install it, or use any editor you like:

```sh
sudo apt install -y nano
```

`init` writes what a first run writes: both ports open on every interface, with the server's own key (`tls = "self"`). Change these three lines:

```toml
name = "Moonward"
public_host = "203.0.113.5"
path = "/var/lib/fenji/fenji.db"
```

- `name`, in `[server]`, is your server's name, which the game and launchers show.
- `public_host`, in `[server]`, is your VPS's public address, or a domain name that points at it. Delete the `#` at the start of its line. Invite links then carry it.
- `path`, in `[store]`, puts the database in the service's data folder.

In nano, Ctrl-O then Enter saves the file, and Ctrl-X leaves.

### The services

Install the server's service, and the backup service with its nightly timer. They're in the repository's `deploy/systemd` at the release's tag. Then start them:

```sh
sudo curl -fL --output-dir /etc/systemd/system \
  -O https://raw.githubusercontent.com/theintonerzero/fenji/v$VERSION/deploy/systemd/fenji.service \
  -O https://raw.githubusercontent.com/theintonerzero/fenji/v$VERSION/deploy/systemd/fenji-backup.service \
  -O https://raw.githubusercontent.com/theintonerzero/fenji/v$VERSION/deploy/systemd/fenji-backup.timer
sudo systemctl daemon-reload
sudo systemctl enable --now fenji fenji-backup.timer
systemctl is-active fenji
```

The last command says `active`. The service runs the server as the `fenji` user, and restarts it 5 seconds after a crash. Its data folder is for that user alone; the rest of the system is read-only to the server, home folders are hidden from it, and it gets none of what it never needs, such as privileges, devices or the kernel's settings. It gives the server 4,096 open files. The timer backs the database up each night at 04:00 (section 6).

Under systemd, the server's log goes to the system's journal. To follow it (Ctrl-C stops):

```sh
sudo journalctl -u fenji -f
```

### The firewall

Let in SSH and the server's two ports, then turn the firewall on:

```sh
sudo apt install -y ufw
sudo ufw allow 22/tcp
sudo ufw allow 7800/tcp
sudo ufw allow 7801/tcp
sudo ufw enable
```

Allow SSH first (port 22, or the port you moved it to), or `ufw enable` shuts you out. If your provider also has a firewall in its control panel, open the same ports there. What open ports expose is in section 3.

### Invite players, and attach the console

Each console command runs at the server with `console --run`, as the `fenji` user. Run the server's other commands as `fenji` too (section 2's "Commands outside the console"): the data folder is that user's alone, and as root they refuse, `init` above aside.

```sh
sudo -u fenji fenji-server --config /etc/fenji/fenji.toml console --run invite
```

```
link (public_host): https://203.0.113.5:7801/#invite=K4QW-7TPM-2XHN-9CRB&pin=8pS1fXyQe0VnJ4cLr6TgWm2HbKd7uZaN3oYiE5qRtUw
server link (public_host): https://203.0.113.5:7801/#pin=8pS1fXyQe0VnJ4cLr6TgWm2HbKd7uZaN3oYiE5qRtUw
code: K4QW-7TPM-2XHN-9CRB
```

Send the invite link (section 2). The server link is for players who already have an account, and `console --run link` shows it without making an invite. To play yourself, invite yourself too, and make your account an op:

```sh
sudo -u fenji fenji-server --config /etc/fenji/fenji.toml console --run "op Wen"
```

To use the console itself, with the log, attach to it:

```sh
sudo -u fenji fenji-server --config /etc/fenji/fenji.toml console
```

Every command of section 2 works there. Ctrl-C, `exit` or `quit` leave the console, and the server runs on. `stop` stops the server, which then stays stopped until `sudo systemctl start fenji`, or the next boot.

### With a domain name: Caddy

With a domain name, Caddy can serve the account API on the usual HTTPS port, with a certificate from Let's Encrypt, so that your links have no port and no pin: `https://play.example.net/#invite=…`. Set it up before you invite players, or send each player your new address afterwards: with Caddy, your server's link is its address alone, `https://play.example.net`.

1. Point the name at your VPS (a DNS A record), and install Caddy as [Caddy's guide](https://caddyserver.com/docs/install) says, which sets it up as a service. Fenji's reference setup uses Caddy 2.11.
2. Open ports 80 and 443, which Let's Encrypt and Caddy need, before Caddy asks for its certificate:

   ```sh
   sudo ufw allow 80/tcp
   sudo ufw allow 443/tcp
   ```

3. Install Fenji's `Caddyfile` for the version you run, put your name in place of `play.example.net` in it, and reload Caddy:

   ```sh
   VERSION=0.5.0
   sudo curl -fL -o /etc/caddy/Caddyfile https://raw.githubusercontent.com/theintonerzero/fenji/v$VERSION/deploy/caddy/Caddyfile
   sudo nano /etc/caddy/Caddyfile
   sudo systemctl reload caddy
   ```

4. In `/etc/fenji/fenji.toml`'s `[server]`, change the `api_bind` and `trusted_proxies` lines that are there, delete the `#` before `public_api` and put your name in it, and put a `#` before `tls = "self"`, since Caddy holds the certificate now. Change each line where it is: a setting written twice stops the server from starting.

   ```toml
   api_bind = "127.0.0.1:7801"
   public_api = "https://play.example.net"
   trusted_proxies = ["127.0.0.1"]
   ```

   `api_bind` on 127.0.0.1 lets only Caddy, on the same machine, reach the account API. `public_api` is where invite links send players' launchers. `trusted_proxies` makes the server believe Caddy about each player's own address: without it, every request seems to come from Caddy, and all your players share one address's limits. `public_host` can go too: players' games then connect to the name they reached the API at.
5. Close port 7801, which Caddy now stands in front of, and restart the server:

   ```sh
   sudo ufw delete allow 7801/tcp
   sudo systemctl restart fenji
   ```

Behind Caddy, every player's connection reaches the server as one of Caddy's, so the server's limit of 16 open connections for each address no longer holds them apart, and one client could take all 256 the account API keeps. Set a limit for each client in Caddy. Caddy's standard build has none, but the [caddy-ratelimit](https://github.com/mholt/caddy-ratelimit) module adds one. Add it to your Caddy, and restart Caddy:

```sh
sudo caddy add-package github.com/mholt/caddy-ratelimit
sudo systemctl restart caddy
```

An upgrade of Caddy's package puts its standard build back, without the module: add it again after one, or set up a build of your own as [Caddy's guide](https://caddyserver.com/docs/build) shows. Then add a `rate_limit` block to your site in `/etc/caddy/Caddyfile`, and `sudo systemctl reload caddy`. A launcher asks a few times a minute, so this limit leaves a household of players room to spare:

```
play.example.net {
	rate_limit {
		zone per_player {
			key {remote_host}
			events 120
			window 1m
		}
	}
	reverse_proxy 127.0.0.1:7801
}
```

## 5. Updating

Read the new release's notes first, for what changed. Updating replaces the program; your settings and data stay as they are.

### On a VPS

Turn maintenance on, so that players can't sign in while you update, and those in the game read why:

```sh
sudo -u fenji fenji-server --config /etc/fenji/fenji.toml console --run "maintenance on 15m Updating Fenji"
```

Download the new version, as in section 4, with its number:

```sh
VERSION=0.5.1
TARGET=$(uname -m)-unknown-linux-musl
curl -fLO https://github.com/theintonerzero/fenji/releases/download/v$VERSION/fenji-server-$VERSION-$TARGET.tar.gz
curl -fLO https://github.com/theintonerzero/fenji/releases/download/v$VERSION/SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
tar -xzf fenji-server-$VERSION-$TARGET.tar.gz
```

Stop the server, put the new program in place, and start it:

```sh
sudo systemctl stop fenji
sudo install -m 755 fenji-server-$VERSION-$TARGET/fenji-server /usr/local/bin/fenji-server
sudo systemctl start fenji
```

`systemctl stop` stops the server as the console's `stop` does, letting every player go and saving where each one stood, and returns once it has stopped, within 30 seconds.

Check that it runs the new version, still in maintenance, which lasts through restarts:

```sh
systemctl is-active fenji
fenji-server --version
sudo -u fenji fenji-server --config /etc/fenji/fenji.toml console --run maintenance
```

Admins can sign in during maintenance, so sign in with your own account and look around. Then open the server again:

```sh
sudo -u fenji fenji-server --config /etc/fenji/fenji.toml console --run "maintenance off"
```

### On your own computer

The update goes into your server's folder, so that its settings, its data and every path stay as they are, a scheduled backup's included (section 6).

1. In the console, type `maintenance on 15m Updating Fenji`, then `backup`, then `stop`.
2. Download the new version's archive (section 2), and unpack it anywhere but your server's folder, such as your Downloads folder.
3. Copy everything in the new version's folder into your server's folder, replacing the old files. The archive holds no `fenji.toml` and no `data` folder, so yours stay.
   - On Linux and macOS, delete the old program first, then copy. On macOS, a program copied into the old one's file can be killed as it starts, as macOS remembers the old program's signature; on Linux, a copy can't replace a program something still runs ("Text file busy"). In these two lines, put the new version's folder in place of `~/Downloads/fenji-server-0.5.1-x86_64-unknown-linux-musl`, and your server's folder in place of `~/fenji-server`, if yours are elsewhere or named otherwise:

     ```sh
     rm ~/fenji-server/fenji-server
     cp -R ~/Downloads/fenji-server-0.5.1-x86_64-unknown-linux-musl/. ~/fenji-server/
     ```

   - On Windows, copy the new folder's files into your server's folder in Explorer, and choose "Replace the files in the destination".
4. Start the start script. Windows and macOS may ask again, as on the first start.
5. Check that it runs the new version: the console's status line shows its number, such as `v0.5.1`, and `maintenance` shows it's still on. Then type `maintenance off`.

## 6. Backups and restoring

**What a backup holds:** the database, with the accounts and their password hashes, the invites, the characters and where they stood, the ops and bans, and maintenance and the console's message of the day. It doesn't hold the server's key, the `tls` folder in the data folder, or its settings, `fenji.toml`, which seldom change: keep a copy of each somewhere safe too. Without the key, the server makes a new one, and every player needs a new link. Backups and the key are as private as the server: keep them that way.

**Where backups go:** `backups` in the data folder (`data/backups` in your server's folder, or `/var/lib/fenji/backups` on the reference VPS), as `fenji-<date>-<time>.db`, named for the time in UTC. Each new copy is checked before older ones are deleted, and the last 14 are kept. A copy takes its name only once it has passed that check: one cut short, such as by a restart as it's made, never passes for a backup, and the next backup clears it away. One backup is made at a time; a second one started meanwhile says so.

### Making backups

- The console's `backup` makes one now, and so does the `backup` command outside the console (section 2), with `--keep <n>` to keep another number. Both are safe while the server runs. On the reference VPS, run it as the `fenji` user; as root it refuses:

  ```sh
  sudo -u fenji fenji-server --config /etc/fenji/fenji.toml backup
  ```

- On the reference VPS, the timer makes one each night at 04:00, or up to half an hour later. `systemctl list-timers fenji-backup.timer` shows when the next one runs, and this makes one now, as the timer does:

  ```sh
  sudo systemctl start fenji-backup
  sudo ls -l /var/lib/fenji/backups
  ```

- On your own computer, let the system run the `backup` command at a time your computer is on. On Linux and macOS, `crontab -e` opens your scheduled commands; add a line like this, with your server's folder:

  ```
  0 4 * * * /home/wen/fenji-server/fenji-server --config /home/wen/fenji-server/fenji.toml backup
  ```

  On Windows, Task Scheduler runs it; in a Command Prompt, with your server's folder:

  ```bat
  schtasks /Create /TN "Fenji backup" /SC DAILY /ST 04:00 /TR "\"C:\Users\Wen\fenji-server\fenji-server.exe\" --config \"C:\Users\Wen\fenji-server\fenji.toml\" backup"
  ```

  An update into the same folder keeps these working (section 5).

### Keeping copies elsewhere

A backup on the server's own disk doesn't survive losing that disk, or the VPS. Now and then, copy the backups and the key to another machine. On the reference VPS, this packs both into a file only you can read, made private before anything is written to it:

```sh
(umask 077; sudo tar -C /var/lib/fenji -czf fenji-backup.tar.gz backups tls)
sudo chown "$(id -un)" fenji-backup.tar.gz
```

Then fetch it from your own computer, with your server's address, and keep it private there too:

```sh
scp wen@203.0.113.5:fenji-backup.tar.gz .
chmod 600 fenji-backup.tar.gz
```

### Restoring

Restoring brings back the database as it was when the copy was made: whatever happened since, such as new accounts, is lost. Stop the server, put the copy in place of the database, delete the database's `-wal` and `-shm` files if they're there, and start it again. On the reference VPS, with the copy you choose:

```sh
sudo systemctl stop fenji
sudo ls /var/lib/fenji/backups
sudo -u fenji cp /var/lib/fenji/backups/fenji-20261016-040312.db /var/lib/fenji/fenji.db
sudo rm -f /var/lib/fenji/fenji.db-wal /var/lib/fenji/fenji.db-shm
sudo systemctl start fenji
```

On your own computer: stop the server, copy the backup over `data/fenji.db`, delete `data/fenji.db-wal` and `data/fenji.db-shm` if they're there, and start it.

**Moving to another VPS.** Set the server up there as in section 4, and bring your settings across: in the new `/etc/fenji/fenji.toml`, set `name`, `motd`, `host`, `max_players` and `admins` as they were in the old one, with the new VPS's address in `public_host`. Copy your `fenji-backup.tar.gz` to it. Then stop the server, unpack the copy into the data folder, key included, give its files to the `fenji` user, put a backup in place of the database, and start it again:

```sh
sudo systemctl stop fenji
sudo rm -rf /var/lib/fenji/tls
sudo tar -C /var/lib/fenji -xzf fenji-backup.tar.gz
sudo chown -R fenji:fenji /var/lib/fenji/backups /var/lib/fenji/tls
sudo ls /var/lib/fenji/backups
sudo -u fenji cp /var/lib/fenji/backups/fenji-20261016-040312.db /var/lib/fenji/fenji.db
sudo rm -f /var/lib/fenji/fenji.db-wal /var/lib/fenji/fenji.db-shm
sudo systemctl start fenji
```

With its old key, the server keeps its pin. If the new VPS has another address, point your domain name at it, or send players your server's link with the new address (section 2).

## 7. Troubleshooting

**Friends can't connect.**
- Is the server running? The console's status line says `online`; on the reference VPS, `systemctl is-active fenji` says `active`, and `sudo journalctl -u fenji` shows its log.
- Both ports, 7800 and 7801, must reach the server: forwarded in the router with the same numbers (section 3), and open in every firewall on the way, your computer's and your VPS provider's included. Port 7801 alone lets players sign in but not into the game.
- Did they use the right link? Friends on other networks need your public address in it, or your VPN's (section 3).
- Your public address may have changed (section 3).

**Windows says "Windows protected your PC"** (SmartScreen), because the server isn't signed by Microsoft: choose More info, then Run anyway.

**macOS says it can't check the server for malware** (Gatekeeper), because the server isn't signed by Apple: close the message, then in System Settings, Privacy & Security, choose Open Anyway beside its name. It may ask for `start-server.command`, then for `fenji-server`. Alternatively, in Terminal, this lets the whole folder run, with your server's folder:

```sh
xattr -dr com.apple.quarantine ~/fenji-server
```

**Windows says Smart App Control blocked the server, with no Run anyway.** Smart App Control, on from the start on some new Windows 11 PCs, blocks programs that aren't signed, and the server isn't. The only way past it is to turn it off, in Windows Security, App & browser control, Smart App Control settings. On many PCs Windows can't turn it on again without a reset of Windows, so weigh that first, or host on another computer: a Linux or macOS one, or a VPS (section 4).

**A player's launcher says "That isn't a server address or an invite link. Paste the link your host sent."** in the desktop launcher, or "that isn't an invite code or link" from `fenji-launch`. What they pasted isn't the whole link: part of it was left out as it was copied, from the console or by a chat app. Copy it again from its first letter to its last (section 2's "Copying a link"), or make it with `./fenji-server invite create`, which prints each link on one line, and send that.

**"a server is already using this data folder (…)".** Another `fenji-server` is running with the same data folder: in another window, in `tmux`, or as the reference deployment's service. Use that one (on Linux and macOS, `fenji-server console` attaches to it), or stop it first. The system lets go of a server's lock however it ends, so this never means a stale lock.

**"the data folder … is fenji's, and this runs as root: run fenji-server as fenji (sudo -u fenji fenji-server …)…".** The server, or one of its commands, was started as another user than the one whose data folder it is, often with `sudo` alone. Run it as the user it names: on the reference VPS, `sudo -u fenji fenji-server --config /etc/fenji/fenji.toml <command>`. A file another user made in the data folder would be theirs, and the server couldn't use it.

**"every user may write in the data folder …: take that away with chmod o-w …".** Anyone on the computer could replace the server's files there. Run the `chmod` command it gives.

**A player's launcher says the server's key has changed:** "Moonward's key has changed. Ask Wen for a new link." in the desktop launcher, with your server's name and yours, or "this server's key has changed: ask your host for a new link" from `fenji-launch`. The server answering at that address has another key than the one in the link the player first used. Either the server lost its `data/tls` folder and made a new key, or something else is answering at that address. If you restore the folder from your copy (section 6), the old links work again. Otherwise, send each player your server's link without an invite, which `link` shows (section 2). Players should hear from you before they trust a new key.

**A player's launcher says the server uses its own key:** "Moonward uses its own key. Open the link Wen sent you." in the desktop launcher, or "this server uses its own key: open the link your host sent" from `fenji-launch`. The player added your address without its pin: send them your server's link (section 2).

**"… holds the server's key but other users can open it (mode 755): run `chmod 700 …`".** The server refuses a key folder that other users can open. Run the command it gives. On a FAT or exFAT drive, folders can't be made private: move the server's folder to your computer's own disk.

**A command says "can't read fenji.toml: there's none in this folder (…)".** It was run outside the server's folder, without `--config`. Run it in the server's folder, or give `--config` with the server's `fenji.toml`.

**`fenji-server console` says "no server is running with this data folder (…)".** The server isn't running, or the console uses other settings: give it the same `--config` as the server, or run it in the server's folder.

**It says "… its console socket is another user's; attach as the user the server runs as".** Attach as that user: on the reference VPS, `sudo -u fenji fenji-server --config /etc/fenji/fenji.toml console`.

**It says "can't attach to the server: the path of its console socket, …, is too long for a socket here".** The server said as much in its log when it started, and runs without a socket. Move the data folder to a shorter path, or start the server from its own folder, as the start scripts do (section 2).

**The service won't start, and `systemctl status fenji` shows `status=217/USER` or `status=226/NAMESPACE`.** `sudo journalctl -u fenji` says why, such as "Failed to keep CAP_SYS_ADMIN: Operation not permitted". Your VPS is most likely a container itself (LXC, OpenVZ and the like), whose systemd lacks a privilege that part of the service's sandbox needs: the settings that filter the server's system calls. Turn those off, for both services, and start the server again:

```sh
sudo mkdir -p /etc/systemd/system/fenji.service.d /etc/systemd/system/fenji-backup.service.d
sudo tee /etc/systemd/system/fenji.service.d/container.conf /etc/systemd/system/fenji-backup.service.d/container.conf > /dev/null <<'EOF'
[Service]
LockPersonality=no
MemoryDenyWriteExecute=no
PrivateDevices=no
ProtectClock=no
ProtectHostname=no
ProtectKernelLogs=no
ProtectKernelModules=no
ProtectKernelTunables=no
RestrictNamespaces=no
RestrictRealtime=no
RestrictSUIDSGID=no
SystemCallArchitectures=
EOF
sudo systemctl daemon-reload
sudo systemctl restart fenji
systemctl is-active fenji
```

`sudo systemctl edit fenji` and `sudo systemctl edit fenji-backup` take the same lines, one service at a time. The server still runs as its own user, without privileges, and keeps the rest of its sandbox where the VPS allows it. If the journal says "Failed to set up mount namespacing" instead, turn off the settings that need a mount namespace too, and start it again:

```sh
printf 'ProtectSystem=no\nProtectHome=no\nPrivateTmp=no\nProtectControlGroups=no\n' | sudo tee -a /etc/systemd/system/fenji.service.d/container.conf /etc/systemd/system/fenji-backup.service.d/container.conf > /dev/null
sudo systemctl daemon-reload
sudo systemctl restart fenji
```

To undo either, delete the two `container.conf` files, then `sudo systemctl daemon-reload` and restart the server.

**The server won't start, and its log says "can't listen on 0.0.0.0:7800".** Another program, or another Fenji server, uses that port.

**"there's no config at /etc/fenji/fenji.toml".** The service's settings are missing: write them with `sudo fenji-server --config /etc/fenji/fenji.toml init` (section 4).

**Players are told the server is in maintenance:** "Moonward is in maintenance: Back soon." in the desktop launcher, or "the server is in maintenance: Back soon" from `fenji-launch`, with your message. Maintenance is on, and stays on through restarts: `maintenance off` turns it off.
