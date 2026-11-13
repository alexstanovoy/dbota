#!/usr/bin/env bash
# Every #[ignore]d test of this repository must be run by a step of the gate: a test nobody runs is a test nobody notices
# failing. Fails and names each file that holds an ignored test no step claims. A new ignored test needs its step (or a
# line here saying which step runs it).
#
#   tools/ignored_audit.sh
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here"
bad=0
while IFS= read -r file; do
    case "$file" in
        crates/dbota_import/tests/* | crates/dbota_import/src/*) ;; # gate step install-tests: cargo test -p dbota_import -- --ignored
        src/tip_content.rs) ;;          # gate step tip-content
        src/e2e/memory.rs) ;;           # run in a process of its own by its test clients_one_after_another_leave_no_memory_behind
        src/fog/clarity_render.rs | src/overlay/render_tests.rs | src/minimap/render_tests.rs | src/e2e/ground_effect_pixels.rs)
            case "$file" in
                src/fog/clarity_render.rs) test_name=fog::clarity_render::full_visual_clarity_reaches_the_ground_and_figure_pixels ;;
                src/overlay/render_tests.rs) test_name=overlay::render_tests::indicator_strokes_are_two_pixels_at_a_fixed_camera ;;
                src/e2e/ground_effect_pixels.rs) test_name=e2e::ground_effect_pixels::objective_ground_effects_have_their_authored_shapes_by_day_and_by_night ;;
                src/minimap/render_tests.rs) test_name=minimap::render_tests::minimap_camera_ping_and_teleport_strokes_halve_physical_widths_above_one_pixel ;;
            esac
            ignored=$(awk '
                /^[[:space:]]*#\[ignore/ { count++; pending=1 }
                pending && /^[[:space:]]*fn / { name=$2; sub(/\(.*/, "", name); print name; pending=0 }
                END { if (count != 1) print "unexpected ignored count: " count }
            ' "$file")
            if [ "$ignored" != "${test_name##*::}" ] || ! grep -Fq -- "--bin dbota $test_name" tools/visual_check.sh; then
                echo "ignored_audit: $file must route exactly its named render contract"
                bad=1
            fi
            ;;
        src/tip_band.rs)
            test_name=tip_band::render::each_band_draws_its_art_from_the_left_end_unscaled_at_every_scale
            ignored=$(awk '
                /^[[:space:]]*#\[ignore/ { count++; pending=1 }
                pending && /^[[:space:]]*fn / { name=$2; sub(/\(.*/, "", name); print name; pending=0 }
                END { if (count != 1) print "unexpected ignored count: " count }
            ' "$file")
            if [ "$ignored" != "${test_name##*::}" ] || ! grep -Fq -- "--bin dbota $test_name" tools/visual_check.sh; then
                echo "ignored_audit: $file must route exactly its named render contract"
                bad=1
            fi
            ;;
        tests/*.rs)                     # tools/visual_check.sh, one `--test NAME` per file
            name=$(basename "$file" .rs)
            grep -q -- "--test $name\b" tools/visual_check.sh || { echo "ignored_audit: $file has ignored tests that tools/visual_check.sh does not run"; bad=1; } ;;
        *) echo "ignored_audit: $file has ignored tests that no gate step runs"; bad=1 ;;
    esac
done < <(git ls-files '*.rs' | xargs grep -l '#\[ignore')
[ "$bad" = 0 ] && echo "ignored_audit: every ignored test has a step"
exit "$bad"
