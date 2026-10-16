#!/bin/sh
# Fenji opens this in a terminal so that you sign in to Steam in DepotDownloader's own window
# (launcher spec §3.6). Fenji reads only the progress lines and the exit status it leaves in the run folder.
run=$1 dd=$2 folder=$3 list=$4
echo $$ > "$run/pgid.tmp" && mv "$run/pgid.tmp" "$run/pgid"
echo "Fenji: DepotDownloader fetches Swords of Legends Online, build 10015727, from your Steam library."
echo "Sign in by scanning the QR code below with the Steam app. Fenji never sees your Steam sign-in."
echo
set -- -app 1418100 -depot 1418101 -manifest 5002037851744157048 -os windows -osarch 64 -qr -dir "$folder"
if [ -n "$list" ]; then set -- "$@" -filelist "$list" -validate; fi
{ "$dd" "$@"; echo $? > "$run/status.tmp"; } 2>&1 | while IFS= read -r line; do
  printf '%s\n' "$line"
  case $line in *%\ *) printf '%s\n' "$line" >> "$run/progress" ;; esac
done
mv "$run/status.tmp" "$run/status"
echo
if [ "$(cat "$run/status")" = 0 ]; then echo "Done. Fenji checks the files now."; else echo "DepotDownloader stopped. Fenji says what to try next."; fi
printf 'You can close this window. '
read -r _
