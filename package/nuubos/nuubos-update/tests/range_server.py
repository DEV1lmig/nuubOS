"""Static HTTP server for tests/update.sh that honours "Range: bytes=N-"
like GitHub Releases (http.server answers every range with 200 and the
whole file, which hides what a resume keeps). Run: range_server.py PORT,
from the directory to serve."""
import os
import sys
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer


class Handler(SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def send_head(self):
        spec = self.headers.get("Range", "")
        path = self.translate_path(self.path)
        if not spec.startswith("bytes=") or not os.path.isfile(path):
            return super().send_head()
        start = int(spec[6:].split("-")[0])
        size = os.path.getsize(path)
        if start >= size:
            self.send_response(416)
            self.send_header("Content-Range", "bytes */%d" % size)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return None
        f = open(path, "rb")
        f.seek(start)
        self.send_response(206)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Range", "bytes %d-%d/%d" % (start, size - 1, size))
        self.send_header("Content-Length", str(size - start))
        self.end_headers()
        return f


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
