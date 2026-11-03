#!/usr/bin/env bash
# Renders Tiny Cove footage with the game's deterministic `--record` mode: game time advances
# exactly 1/30 s per frame and every frame is captured, so the videos play at true game speed
# even where rendering is slow (software Vulkan in CI or a container).
#
# Output (default tiny-cove/docs/footage/):
#   tiny-cove.mp4     every clip back to back: H.264, yuv420p, CRF 29 (env CRF overrides),
#                     30 fps, 1280x720 native pixels, encoded once from the lossless clips
#   tiny-cove.gif     README preview, 640x360 at 15 fps (sped-up spans of the clips, <= 8 MB)
#   stills/<clip>-N.png  lossless frames picked from each clip
# Each clip's own preview mp4 and lossless copy stay in the work folder: they would only repeat
# tiny-cove.mp4 in the repository.
#
# usage: tools/footage.sh [options] [clip ...]
#   clips: day harbour weather night build (default: all of them)
#   -w, --work DIR     frames and logs (default: ${TMPDIR:-/tmp}/tiny-cove-footage)
#   -o, --out DIR      output folder (default: <tiny-cove>/docs/footage)
#   -b, --bin PATH     game binary (default: ${CARGO_TARGET_DIR:-../target}/release/tiny-cove)
#   -s, --seconds S    cap each scripted clip at S seconds, for quick checks
#       --keep-frames  keep the PNG frames in the work folder
#       --no-gif       skip the preview GIF
#       --no-concat    skip tiny-cove.mp4
#
# Without $DISPLAY the game runs under xvfb-run (1280x720) on Mesa lavapipe if it is installed.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
FPS=30
ALL_CLIPS=(day harbour weather night build)
WORK=${TMPDIR:-/tmp}/tiny-cove-footage
OUT=$here/docs/footage
BIN=${CARGO_TARGET_DIR:-$here/../target}/release/tiny-cove
CAP=""
CRF=${CRF:-29}
KEEP=0
GIF=1
CONCAT=1
GIF_MAX_BYTES=$((8 * 1024 * 1024))

usage() { sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d; s/^# \{0,1\}//'; }

clips=()
while (($#)); do
    case $1 in
        -w | --work | -o | --out | -b | --bin | -s | --seconds)
            (($# >= 2)) || { echo "footage: $1 needs a value" >&2; usage >&2; exit 2; }
            case $1 in
                -w | --work) WORK=$2 ;;
                -o | --out) OUT=$2 ;;
                -b | --bin) BIN=$2 ;;
                -s | --seconds) CAP=$2 ;;
            esac
            shift 2 ;;
        --keep-frames) KEEP=1; shift ;;
        --no-gif) GIF=0; shift ;;
        --no-concat) CONCAT=0; shift ;;
        -h | --help) usage; exit 0 ;;
        -*) echo "footage: unknown option $1" >&2; usage >&2; exit 2 ;;
        *)
            [[ " ${ALL_CLIPS[*]} " == *" $1 "* ]] || { echo "footage: unknown clip '$1' (clips: ${ALL_CLIPS[*]})" >&2; exit 2; }
            clips+=("$1"); shift ;;
    esac
done
((${#clips[@]})) || clips=("${ALL_CLIPS[@]}")
if [[ -n $CAP ]] && ! awk -v s="$CAP" 'BEGIN { exit !(s + 0 > 0) }'; then
    echo "footage: --seconds expects a positive number" >&2; exit 2
fi
[[ -x $BIN ]] || { echo "footage: no game binary at $BIN (cargo build --release, or pass --bin)" >&2; exit 1; }
for tool in ffmpeg ffprobe; do
    command -v "$tool" >/dev/null || { echo "footage: $tool not found" >&2; exit 1; }
done

RUN=()
if [[ -z ${DISPLAY:-} ]]; then
    command -v xvfb-run >/dev/null || { echo "footage: no \$DISPLAY and no xvfb-run" >&2; exit 1; }
    RUN=(xvfb-run -a -s "-screen 0 1280x720x24")
    lvp=/usr/share/vulkan/icd.d/lvp_icd.json
    if [[ -z ${VK_ICD_FILENAMES:-} && -f $lvp ]]; then
        export VK_ICD_FILENAMES=$lvp WGPU_BACKEND=${WGPU_BACKEND:-vulkan}
    fi
fi
mkdir -p "$WORK" "$OUT/stills"

# Clip scripts. Seconds come last so --seconds can cap them.
clip_args() {
    case $1 in
        # Dawn to night in 12 s with a slow quarter orbit: dawn, noon, golden hour, lit beacons.
        day) echo "--time 5.5 --hours 17 --orbit 60 --seconds 12" ;;
        # Golden-hour close-up of the harbour at real game speed: sea, canoes, gulls, pennants.
        harbour) echo "--time 17.5 --zoom 1.0 --focus 7,2 --yaw 90 --seconds 8" ;;
        # Mid-day weather tour (the game blends each change over a few seconds).
        weather) echo "--time 11 --weathers clear,rain,storm,fog,snow --orbit 45 --seconds 35" ;;
        # Fires, lanterns, window glow and stars.
        night) echo "--time 23 --orbit 30 --seconds 6" ;;
    esac
}

now() { date +%s.%N; }
since() { awk -v a="$1" -v b="$(now)" 'BEGIN { printf "%.1f", b - a }'; }
count_frames() { find "$1" -maxdepth 1 -name 'frame_*.png' | wc -l; }

render() {
    local name=$1 dir=$WORK/$1 log=$WORK/$1.log t0 args n
    rm -rf "$dir"
    mkdir -p "$dir"
    t0=$(now)
    if [[ $name == build ]]; then
        # The scripted --tool-smoke build run with its HUD, recorded frame by frame.
        args=(--tool-smoke "$dir/smoke.png" --record "$dir/frames")
    else
        read -r -a args <<<"$(clip_args "$name")"
        if [[ -n $CAP ]]; then
            args[${#args[@]} - 1]=$(awk -v a="${args[${#args[@]} - 1]}" -v c="$CAP" 'BEGIN { print (c + 0 < a + 0) ? c : a }')
        fi
        args=(--record "$dir/frames" "${args[@]}")
    fi
    echo "footage: $name: rendering ($BIN ${args[*]})"
    if ! "${RUN[@]}" "$BIN" "${args[@]}" >"$log" 2>&1; then
        echo "footage: $name: the game failed, last lines of $log:" >&2
        tail -n 20 "$log" >&2
        exit 1
    fi
    n=$(count_frames "$dir/frames")
    ((n > 0)) || { echo "footage: $name: no frames written" >&2; exit 1; }
    awk -v n="$n" -v s="$(since "$t0")" -v name="$name" \
        'BEGIN { printf "footage: %s: %d frames rendered in %.1f s (%.2f s per frame)\n", name, n, s, s / n }'
}

encode() {
    local name=$1 frames=$WORK/$1/frames t0 n step=1 size vf=() i k
    t0=$(now)
    n=$(count_frames "$frames")
    # The build run is long: play it at 2x by keeping every 2nd frame.
    if [[ $name == build ]] && ((n > 20 * FPS)); then step=2; fi
    local filters=()
    ((step > 1)) && filters+=("select='not(mod(n\\,$step))'" "setpts=N/($FPS*TB)")
    size=$(ffprobe -v error -select_streams v:0 -show_entries stream=width,height -of csv=p=0:s=x "$frames/frame_00001.png")
    # Native pixels: only a window that is not 1280x720 (HiDPI) gets a nearest-neighbour fit.
    [[ $size == 1280x720 ]] || filters+=("scale=1280:720:flags=neighbor")
    if ((${#filters[@]})); then
        local IFS=,
        vf=(-vf "${filters[*]}")
        unset IFS
    fi
    ffmpeg -hide_banner -loglevel error -y -framerate "$FPS" -i "$frames/frame_%05d.png" "${vf[@]}" \
        -c:v libx264 -preset slow -tune animation -crf "$CRF" -pix_fmt yuv420p -r "$FPS" -movflags +faststart "$WORK/$name.mp4"
    # A lossless RGB copy for the GIF: H.264 4:2:0 noise defeats GIF frame differencing and
    # roughly doubles its size.
    ffmpeg -hide_banner -loglevel error -y -framerate "$FPS" -i "$frames/frame_%05d.png" "${vf[@]}" \
        -c:v libx264rgb -qp 0 -preset ultrafast -r "$FPS" "$WORK/$name.lossless.mkv"
    # Stills straight from the lossless frames; the build run's last still is the finished build.
    local at=(20 55 90)
    [[ $name == build ]] && at=(35 70 100)
    rm -f "$OUT/stills/$name"-*.png
    for i in "${!at[@]}"; do
        k=$(((n * at[i] + 99) / 100))
        ((k < 1)) && k=1
        cp "$(printf '%s/frame_%05d.png' "$frames" "$k")" "$OUT/stills/$name-$((i + 1)).png"
    done
    echo "footage: $name: encoded $WORK/$name.mp4 ($(du -h "$WORK/$name.mp4" | cut -f1), ${step}x, $(since "$t0") s), ${#at[@]} stills"
    ((KEEP)) || rm -rf "$frames"
}

concat() {
    local list=$WORK/concat.txt t0 name
    t0=$(now)
    : >"$list"
    for name in "${ALL_CLIPS[@]}"; do
        [[ -f $WORK/$name.lossless.mkv ]] && printf "file '%s'\n" "$WORK/$name.lossless.mkv" >>"$list"
    done
    [[ -s $list ]] || return 0
    # One encode from the lossless clips (not a stream copy of the previews): a single rate
    # control pass over the whole video keeps it near 30 MB at CRF 29.
    ffmpeg -hide_banner -loglevel error -y -f concat -safe 0 -i "$list" \
        -c:v libx264 -preset slow -tune animation -crf "$CRF" -pix_fmt yuv420p -r "$FPS" -movflags +faststart "$OUT/tiny-cove.mp4"
    echo "footage: tiny-cove.mp4: $(ffprobe -v error -show_entries format=duration -of csv=p=0 "$OUT/tiny-cove.mp4") s, $(du -h "$OUT/tiny-cove.mp4" | cut -f1) ($(since "$t0") s)"
}

# Spans of the clips that go into the README GIF: "clip start seconds speed" (video time).
# A moving camera costs ~0.8 MB per GIF second, the still harbour shot ~0.25 MB.
# The weather tour holds each weather for 7 s (clear, rain, storm, fog, snow) and a change takes
# ~6 s to blend in, so the GIF takes the last 1.6 s of each one. With the day at 4x this comes
# to ~7.5 MB, a little under the old 15 s weather span at 5x.
GIF_SPANS=("day 0 12 4" "harbour 1 4 1"
    "weather 5.4 1.6 2" "weather 12.4 1.6 2" "weather 19.4 1.6 2" "weather 26.4 1.6 2" "weather 33.4 1.6 2"
    "night 0 3 1.5" "build 2 24 8")
GIF_TRIES=5

make_gif() {
    local t0 scale=1 try span name start dur speed src lossless length inputs=() chains="" labels="" k=0 bytes
    t0=$(now)
    for ((try = 1; try <= GIF_TRIES; try++)); do
        inputs=() chains="" labels="" k=0
        for span in "${GIF_SPANS[@]}"; do
            read -r name start dur speed <<<"$span"
            src=$WORK/$name.mp4 lossless=$WORK/$name.lossless.mkv
            [[ -f $src ]] || continue
            # The lossless copy, unless the MP4 came from a later run.
            [[ -f $lossless && ! $src -nt $lossless ]] && src=$lossless
            # A shortened span keeps its end: the weather fully blended in, the finished build.
            start=$(awk -v a="$start" -v d="$dur" -v s="$scale" 'BEGIN { printf "%.2f", a + d * (1 - s) }')
            dur=$(awk -v d="$dur" -v s="$scale" 'BEGIN { printf "%.2f", d * s }')
            # A clip cut short by --seconds may end before the span starts: take its last
            # seconds instead, so quick runs still make a GIF.
            length=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$src")
            start=$(awk -v a="$start" -v d="$dur" -v l="$length" 'BEGIN { if (a + 0 >= l + 0) a = (l > d) ? l - d : 0; printf "%.2f", a }')
            inputs+=(-ss "$start" -t "$dur" -i "$src")
            chains+="[$k:v]setpts=(PTS-STARTPTS)/$speed,fps=15,scale=640:360:flags=neighbor,setsar=1[v$k];"
            labels+="[v$k]"
            k=$((k + 1))
        done
        ((k)) || return 0
        ffmpeg -hide_banner -loglevel error -y -sws_flags neighbor "${inputs[@]}" -filter_complex \
            "${chains}${labels}concat=n=$k:v=1:a=0,split[a][b];[a]palettegen=max_colors=256:stats_mode=full[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" \
            "$OUT/tiny-cove.gif"
        bytes=$(stat -c %s "$OUT/tiny-cove.gif")
        ((bytes <= GIF_MAX_BYTES)) && break
        ((try < GIF_TRIES)) || break
        echo "footage: tiny-cove.gif is $((bytes / 1024)) KiB, shortening its spans"
        scale=$(awk -v s="$scale" 'BEGIN { print s * 0.75 }')
    done
    if ((bytes > GIF_MAX_BYTES)); then
        echo "footage: tiny-cove.gif is still $((bytes / 1024)) KiB after $GIF_TRIES tries, over the" \
            "$((GIF_MAX_BYTES / 1024)) KiB budget: shorten GIF_SPANS ($OUT/tiny-cove.gif left for a look)" >&2
        exit 1
    fi
    echo "footage: tiny-cove.gif: $((bytes / 1024)) KiB from $k spans ($(since "$t0") s)"
}

total=$(now)
for name in "${clips[@]}"; do
    render "$name"
    encode "$name"
done
((CONCAT)) && concat
((GIF)) && make_gif
echo "footage: done in $(since "$total") s -> $OUT"
