Fenji @VERSION@: a private server for Swords of Legends Online
==============================================================

1. Rename this folder fenji-server: it's your server's folder from now on,
   and each update goes into it, as the hosting guide in step 4 says. Then
   start the server with the start script in it:
   - on Windows, double-click start-server.bat;
   - on macOS, double-click start-server.command, which opens in Terminal;
   - on Linux, open a terminal in this folder (in your file manager,
     right-click the folder and choose Open in Terminal), and run
     ./start-server.sh.
   Windows and macOS may warn about a program from an unknown developer: the
   hosting guide in step 4 shows how to start it anyway. If Windows asks
   whether to let the server through its firewall, allow it. The first start
   writes the server's settings, fenji.toml, and a data folder here.

2. Type help, and press Enter, to see the server's commands.

3. Type invite to make an invite link, and send the link to a friend: select
   it, then copy it with Ctrl-Shift-C on Linux (Ctrl-C there stops the
   server), Cmd-C on macOS or Ctrl-C on Windows.

4. Friends on your network can join now. To let in friends from elsewhere,
   over the internet or a VPN, follow the hosting guide for this version:
   https://github.com/theintonerzero/fenji/blob/v@VERSION@/docs/hosting.md

5. Every player, you included, needs their own legitimate copy of the game:
   on Steam, if it's already in their library, or from Gameforge's launcher
   back when Gameforge distributed it. Fenji doesn't include the game or any
   of its files.

fenji.example.toml describes every setting. Fenji is free software: LICENSE
is its licence, the GNU Affero General Public License, version 3 or later,
and SOURCE.txt says where this version's source code is.
THIRD-PARTY-NOTICES.txt lists the open-source code built into the server,
with its licences.
