"""Regenerates src/sauda/_proto from jugaad-rpc's .proto (the single source of
truth). Run once before `maturin build` / `maturin develop`:

    pip install -r requirements-gen.txt
    python generate.py
"""

import sys
from pathlib import Path

from grpc_tools import protoc

root = Path(__file__).resolve().parent
proto_dir = root.parent / "crates" / "jugaad-rpc" / "proto"
out = root / "src" / "sauda" / "_proto"

out.mkdir(parents=True, exist_ok=True)
(out / "__init__.py").write_text("")

rc = protoc.main(
    [
        "protoc",
        f"-I{proto_dir}",
        f"--python_out={out}",
        f"--grpc_python_out={out}",
        str(proto_dir / "jugaad.proto"),
    ]
)
if rc:
    sys.exit(rc)

# protoc emits a top-level `import jugaad_pb2`, which can't resolve inside a
# package - make it relative.
grpc_file = out / "jugaad_pb2_grpc.py"
text = grpc_file.read_text()
fixed = text.replace(
    "import jugaad_pb2 as jugaad__pb2", "from . import jugaad_pb2 as jugaad__pb2"
)
assert fixed != text, "protoc output changed shape; update the import rewrite"
grpc_file.write_text(fixed)
print(f"Generated stubs in {out}")
