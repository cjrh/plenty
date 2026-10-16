#!/bin/sh
# Regenerates the std.tls test certificates (OpenSSL 3.4 or later). They are
# for loopback tests only: the private keys are public.
set -eu
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
key() { openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$1"; }
ca() {
    openssl req -x509 -new -key "$1" -out "$2" -subj "/CN=$3" \
        -not_before 20240101000000Z -not_after 21240101000000Z \
        -addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign
}
leaf() {
    openssl req -new -key leaf.key -out "$work/leaf.csr" -subj /CN=localhost
    printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n' >"$work/ext"
    openssl x509 -req -in "$work/leaf.csr" -CA "$1" -CAkey "$2" -set_serial "$3" \
        -not_before "$4" -not_after "$5" -extfile "$work/ext" -out "$6"
}
key ca.key
key other-ca.key
key leaf.key
ca ca.key ca.pem "Plenty Test CA"
ca other-ca.key other-ca.pem "Plenty Other CA"
leaf ca.pem ca.key 1 20240101000000Z 21240101000000Z leaf.pem
leaf ca.pem ca.key 2 20000101000000Z 20010101000000Z expired.pem
leaf other-ca.pem other-ca.key 3 20240101000000Z 21240101000000Z untrusted.pem
rm ca.key other-ca.key
