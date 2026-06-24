from fastapi import FastAPI, Response
from fastapi.middleware.cors import CORSMiddleware
from pathlib import Path
from threading import Thread
import uvicorn
from pmtiles.reader import Reader
from functools import lru_cache
import threading



class PMTilesServer:
    def __init__(self, pmtiles_path: Path):
        self.app = FastAPI()

        self.file = open(pmtiles_path, "rb")

        self.lock = threading.Lock()

        def get_bytes(offset, length):
            with self.lock:
                self.file.seek(offset)
                return self.file.read(length)

        self.reader = Reader(get_bytes)

        self.app.add_middleware(
            CORSMiddleware,
            allow_origins=["*"],
            allow_credentials=True,
            allow_methods=["*"],
            allow_headers=["*"],
        )

        reader = self.reader

        @lru_cache(maxsize=20000)
        def get_tile(z, x, y):
            return reader.get(z, x, y)

        @self.app.get("/tiles/{z}/{x}/{y}.pbf")
        def tile(z: int, x: int, y: int):
            tile_data = get_tile(z, x, y)

            if tile_data is None:
                return Response(status_code=204)

            return Response(
                tile_data,
                media_type="application/x-protobuf",
                headers={
                    "Content-Encoding": "gzip",
                    "Cache-Control": "public, max-age=86400, immutable",
                },
            )


class TileServer:
    def __init__(self, pmtiles_path: Path, host="127.0.0.1", port=8080):
        self.host = host
        self.port = port

        self.app = PMTilesServer(pmtiles_path).app

        self.config = uvicorn.Config(
            self.app,
            host=self.host,
            port=self.port,
            log_level="warning",
        )

        self.server = uvicorn.Server(self.config)
        self.thread = None

    def start(self):
        if self.thread and self.thread.is_alive():
            return

        self.thread = Thread(target=self.server.run, daemon=True)
        self.thread.start()

    def stop(self):
        self.server.should_exit = True
        if self.thread:
            self.thread.join(timeout=5)

    @property
    def url(self):
        return f"http://{self.host}:{self.port}/tiles/{{z}}/{{x}}/{{y}}.pbf"
