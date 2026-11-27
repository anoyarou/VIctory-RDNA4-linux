# VIctory settings for the RX 9070 XT under Wine/Proton, as measured on this branch
# (docs/linux-wine.md has the runs behind each one). Sourced by run.sh; variables already set in the
# environment win, so any of them can be changed per launch: VICTORY_RELAY=1 ./run.sh ...
# SETTINGS=0 skips this file.

# The settings of the base's scripts/play.ps1 (Belobug/VIctory-Belobug-AMD `rx9070xt`), not -Safe,
# without ray tracing: 575 -> ~300 ms a frame on this base (docs/linux-wine.md, runs 33-34).
: "${VICTORY_TEXTURE_NO_WAIT:=1}" "${VICTORY_RELAY:=2}" "${VICTORY_WIDE_BACK:=1}" "${VICTORY_CARD_REHASH_RUNS:=1}"
: "${VICTORY_JOIN_COPIES:=1}" "${VICTORY_INDIRECT_WINDOW:=1}" "${VICTORY_RELAY_COPY:=1}" "${VICTORY_CARD_STEPS:=2}"
: "${VICTORY_INDIRECT_BATCH:=2}" "${VICTORY_MAP_THREADS:=1}" "${VICTORY_TITLE_PRIORITIES:=0}" "${VICTORY_CARD_AWAKE:=1}"
: "${VICTORY_SHOW_AFTER:=1}" "${VICTORY_D3D12_DRED:=light}" "${VICTORY_SKIP_PIPELINES:=none}"
: "${VICTORY_THIN:=1}" "${VICTORY_JOIN_PARTS:=8}" "${VICTORY_UPLOAD_AHEAD:=1}"
: "${VICTORY_HAIR:=1}"

# With RELAY=2 a few fetched pieces are torn (oily or flat skin, wrong eyes); the check fetches them
# again at no measured cost (avatarmanz/VIctory-7900XTX#13, #138, #148).
: "${VICTORY_FETCH_CHECK:=1}"

# Ambient stand-in without ray tracing: 0.8 of the estimate, never below 0.6 (rooms stay too dark otherwise).
: "${VICTORY_AMBIENT:=0.8,floor:0.6}"

# Title buffers of 1 MiB and more keep their copy on the card (16 GB cards default to 4 MiB): 75 -> 67 ms
# standing, ~130 -> ~29 MB compared a frame (idea from Belobug/VIctory-Belobug-AMD#444).
# Known cost: a few textures in one colour and some artifacts. Unset it (VICTORY_CARD_BUFFERS_MIN_MB=) for the default.
: "${VICTORY_CARD_BUFFERS_MIN_MB:=1}"

# Shaders' writes into write-combined title memory stay on the card (Belobug/VIctory-Belobug-AMD#477):
# copy-back 27 -> 13 ms, 67 -> 59-61 ms standing.
: "${VICTORY_WC_NO_BACK:=1}"

# Left off on purpose (measured here; see docs/linux-wine.md):
#   VICTORY_PORT_GUARD=1, VICTORY_PORT_JOURNAL=1        plastic-looking clothes, waxy skin
#   VICTORY_SUBMIT_FRAMES=2                             no gain, waxy faces
#   VICTORY_PORT_CONSTANTS/DESCRIPTORS/CBV=1 (#440)     more than twice as slow here, floors too dark
