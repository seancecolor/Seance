#!/usr/bin/env bash
set -euo pipefail

BIN_PATH="${HOME}/.cargo/bin/seance-transmitter"
DESKTOP_DIR="${HOME}/.local/share/applications"
DESKTOP_FILE="${DESKTOP_DIR}/seance-transmitter.desktop"

if [ ! -x "${BIN_PATH}" ]; then
  echo "Binary not found at ${BIN_PATH}. Run 'cargo install --path .' first."
  exit 1
fi

mkdir -p "${DESKTOP_DIR}"
cat > "${DESKTOP_FILE}" <<EOF
[Desktop Entry]
Type=Application
Name=Seance Transmitter
Comment=10-bit DeckLink SDI to Railway HEVC Transmitter
Exec=${BIN_PATH}
Icon=camera-video
Terminal=false
Categories=AudioVideo;Video;
EOF

chmod +x "${DESKTOP_FILE}"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "${DESKTOP_DIR}" || true
echo "Installed desktop launcher: ${DESKTOP_FILE}"
