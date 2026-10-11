#!/bin/bash
# Drives the real game with xdotool under Xvfb. Usage: drive.sh BIN
export DISPLAY=:77
Xvfb :77 -screen 0 1280x720x24 >/dev/null 2>&1 &
XV=$!
sleep 1
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan "$1" --seed 3 > game.log 2>&1 &
G=$!
sleep 20
WID=$(xdotool search --name "Thistle Glade" | head -1)
echo "window $WID"
xdotool windowactivate --sync $WID 2>/dev/null; xdotool windowfocus $WID 2>/dev/null
p() { sleep "${1:-1.5}"; }
key() { xdotool key --window $WID "$1"; p; }
drag() { # x1 y1 x2 y2 [steps]
  xdotool mousemove $1 $2; p 1; xdotool mousedown 1; p 1
  local n=${5:-6}
  for i in $(seq 1 $n); do
    xdotool mousemove $(( $1 + ($3-$1)*i/n )) $(( $2 + ($4-$2)*i/n )); p 0.6
  done
  p 1; xdotool mouseup 1; p 2
}
click() { xdotool mousemove $1 $2; p 1; xdotool click 1; p 2; }
shot() { key F12; p 3; }

key 1                      # wall
drag 350 250 900 250 8
shot
key 2                      # house: drag footprint, raise, click
drag 450 380 600 470 5
xdotool mousemove 600 380; p 2; xdotool click 1; p 3
shot
key 5                      # path through the wall
drag 620 150 640 450 6
key g                      # tree
click 950 450
key c
click 300 450              # clutter
key 0                      # roof tool
key 9                      # window on wall
click 450 230
shot
key ctrl+z
shot
key ctrl+y
key x                      # eraser on tree
click 950 430
shot
kill $G; sleep 1; kill $XV
