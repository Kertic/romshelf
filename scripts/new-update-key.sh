#!/usr/bin/env bash
# Creates the Ed25519 key that signs Romshelf releases.
#
# - The public half goes into crates/romshelf-core/update-key.pub (commit it).
# - The private half goes into the repository's UPDATE_SIGNING_KEY secret, and a backup copy
#   is written outside the repository for you to keep somewhere safe (a password manager).
#
# Builds made before a key change can't verify releases signed with the new key, so a Deck on
# such a build needs one manual install. Run this once, not per release.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
backup="${1:-$HOME/romshelf-update-signing-key.pem}"
pub_file=crates/romshelf-core/update-key.pub

if [ -s "$pub_file" ] && [ "${FORCE:-}" != "1" ]; then
  echo "$pub_file already has a key. Run with FORCE=1 to replace it." >&2
  exit 1
fi
if [ -e "$backup" ]; then
  echo "$backup already exists. Move it away or pass another path." >&2
  exit 1
fi

umask 077
openssl genpkey -algorithm ed25519 -out "$backup"
openssl pkey -in "$backup" -pubout -outform DER | tail -c 32 | base64 > "$pub_file"
gh secret set UPDATE_SIGNING_KEY < "$backup"

echo "Public key written to $pub_file. Commit it."
echo "Private key stored in the UPDATE_SIGNING_KEY secret."
echo "Backup of the private key: $backup. Keep it safe, then delete it from this computer."
