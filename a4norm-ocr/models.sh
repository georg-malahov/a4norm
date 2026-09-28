#!/bin/sh
# The two PP-OCRv5 models and the recognizer's settings (its character list),
# at pinned revisions, checked by sha256. Apache-2.0, PaddlePaddle; not in
# the repository.
#   a4norm-ocr/models.sh [DIR]      (default a4norm-ocr/models)
set -e
DIR=${1:-$(dirname "$0")/models}
HF=https://huggingface.co/PaddlePaddle
DET=$HF/PP-OCRv5_mobile_det_onnx/resolve/e6f4fa85f00e168c862bc462aebca69eef9b3d3d
REC=$HF/latin_PP-OCRv5_mobile_rec_onnx/resolve/89d3a50e2c27e2e7cceeab0e944c25c807d5db4f
mkdir -p "$DIR"
get() { # url file sha256
  if [ -f "$DIR/$2" ] && [ "$(shasum -a 256 "$DIR/$2" | cut -d' ' -f1)" = "$3" ]; then return; fi
  curl -fsSL -o "$DIR/$2.part" "$1"
  got=$(shasum -a 256 "$DIR/$2.part" | cut -d' ' -f1)
  [ "$got" = "$3" ] || { echo "$2: sha256 $got, want $3" >&2; rm -f "$DIR/$2.part"; exit 1; }
  mv "$DIR/$2.part" "$DIR/$2"
}
get "$DET/inference.onnx" det.onnx a431985659dc921974177a95adcfbb90fd9e51989a5e04d70d0b75f597b6e61d
get "$REC/inference.onnx" rec.onnx 7888113072263cb471b93f66dd5e2ad70548dc526fa1ace760d0d973dd121498
get "$REC/inference.yml" rec.yml 0bbe984570f597af3638e50bdf2e8276f3ab26a61966096538b3b0d1849f5c84
ls -l "$DIR"
