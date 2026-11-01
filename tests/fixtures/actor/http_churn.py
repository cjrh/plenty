"""Concurrent HTTPS qualification using only Python's standard library.

Every request opens a fresh TCP/TLS connection. Keep this compatible with the
Python in the pinned glibc 2.28 release images; curl there predates parallel mode.
"""
import concurrent.futures
import http.client
import socket
import ssl
import sys

port = int(sys.argv[1])
context = ssl.create_default_context(cafile=sys.argv[2])


def request(number):
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=5) as raw:
            with context.wrap_socket(raw, server_hostname="localhost") as stream:
                stream.sendall(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                with http.client.HTTPResponse(stream) as response:
                    response.begin()
                    body = response.read()
                    if response.status != 200 or body != b"home":
                        return f"ERROR request={number} status={response.status} body={body!r}"
                    return f"200 request={number}"
    except (OSError, http.client.HTTPException) as error:
        return f"ERROR request={number} {type(error).__name__}: {error}"


with concurrent.futures.ThreadPoolExecutor(max_workers=16) as pool:
    for result in pool.map(request, range(5000)):
        print(result, flush=True)
