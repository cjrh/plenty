#!/bin/sh
# Regenerates the std.tls test certificates and revocation lists (OpenSSL 3.4
# or later). They are for loopback tests only: the private keys are public.
set -eu
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
key() { openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$1"; }
# Authorities sign certificates and revocation lists.
ca() {
    openssl req -x509 -new -key "$1" -out "$2" -subj "/CN=$3" \
        -not_before 20240101000000Z -not_after 21240101000000Z \
        -addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign,cRLSign
}
# issued CA_PEM CA_KEY KEY SERIAL NOT_BEFORE NOT_AFTER SUBJECT EXTENSIONS OUT
issued() {
    openssl req -new -key "$3" -out "$work/req.csr" -subj "/CN=$7"
    printf '%s\nbasicConstraints=CA:FALSE\n' "$8" | tr ';' '\n' >"$work/ext"
    openssl x509 -req -in "$work/req.csr" -CA "$1" -CAkey "$2" -set_serial "$4" \
        -not_before "$5" -not_after "$6" -extfile "$work/ext" -out "$9"
}
server='subjectAltName=DNS:localhost,IP:127.0.0.1;extendedKeyUsage=serverAuth'
client='extendedKeyUsage=clientAuth'
# revoke CA_PEM CA_KEY OUT CERTIFICATE...: a list revoking each certificate.
revoke() {
    authority=$1 signer=$2 out=$3
    shift 3
    : >"$work/index.txt"
    echo 01 >"$work/crlnumber"
    printf '[ca]\ndefault_ca=d\n[d]\ndatabase=%s\ncrlnumber=%s\ndefault_md=sha256\n' \
        "$work/index.txt" "$work/crlnumber" >"$work/ca.cnf"
    for certificate in "$@"; do
        openssl ca -config "$work/ca.cnf" -keyfile "$signer" -cert "$authority" -revoke "$certificate"
    done
    openssl ca -config "$work/ca.cnf" -keyfile "$signer" -cert "$authority" \
        -gencrl -crldays 36500 -out "$out"
}
always='20240101000000Z 21240101000000Z'
key ca.key
key other-ca.key
key client-ca.key
key leaf.key
key client.key
ca ca.key ca.pem "Plenty Test CA"
ca other-ca.key other-ca.pem "Plenty Other CA"
ca client-ca.key client-ca.pem "Plenty Client CA"
# shellcheck disable=SC2086
{
    issued ca.pem ca.key leaf.key 1 $always localhost "$server" leaf.pem
    issued ca.pem ca.key leaf.key 2 20000101000000Z 20010101000000Z localhost "$server" expired.pem
    issued other-ca.pem other-ca.key leaf.key 3 $always localhost "$server" untrusted.pem
    issued ca.pem ca.key leaf.key 4 $always second.test \
        'subjectAltName=DNS:second.test;extendedKeyUsage=serverAuth' second.pem
    issued ca.pem ca.key leaf.key 5 $always localhost "$server" revoked.pem
    issued client-ca.pem client-ca.key client.key 1 $always "Plenty Client" "$client" client.pem
    issued client-ca.pem client-ca.key client.key 2 $always "Plenty Revoked Client" "$client" revoked-client.pem
}
revoke ca.pem ca.key ca.crl.pem revoked.pem
revoke client-ca.pem client-ca.key client-ca.crl.pem revoked-client.pem
rm ca.key other-ca.key client-ca.key
