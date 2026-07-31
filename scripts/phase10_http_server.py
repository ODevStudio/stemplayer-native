import argparse
import functools
import http.server
import ssl
import threading


class Handler(http.server.SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    failures = set()
    lock = threading.Lock()

    def do_GET(self):
        if self.path == "/redirect":
            self.send_response(302)
            self.send_header("Location", "/stemset.json")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if self.path == "/oversize":
            body = b"x" * (256 * 1024 + 1)
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path == "/fixture/bass/segment-0.m4a":
            with self.lock:
                first = self.path not in self.failures
                self.failures.add(self.path)
            if first:
                self.send_response(503)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
        super().do_GET()

    def log_message(self, format, *args):
        return


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("root")
    parser.add_argument("port", type=int)
    parser.add_argument("--certificate")
    parser.add_argument("--key")
    args = parser.parse_args()
    handler = functools.partial(Handler, directory=args.root)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), handler)
    if args.certificate and args.key:
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(args.certificate, args.key)
        server.socket = context.wrap_socket(server.socket, server_side=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
