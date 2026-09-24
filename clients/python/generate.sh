#!/usr/bin/env bash
# Regenerates the Python gRPC stubs from jugaad-rpc's own .proto file (the
# single source of truth - nothing is duplicated here).
set -euo pipefail
cd "$(dirname "$0")"

mkdir -p generated
python -m grpc_tools.protoc \
  -I ../../crates/jugaad-rpc/proto \
  --python_out=generated \
  --grpc_python_out=generated \
  --pyi_out=generated \
  ../../crates/jugaad-rpc/proto/jugaad.proto

# protoc's generated grpc file imports its message module as a top-level
# "import jugaad_pb2", which only works if "generated" is on sys.path -
# client.py adds it, so no package __init__.py is needed here.
echo "Generated stubs in $(pwd)/generated"
