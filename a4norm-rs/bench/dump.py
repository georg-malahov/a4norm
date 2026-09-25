"""Run the a4norm script and keep the page around the three stages the Rust
crate reproduces, as the oracle to compare against:

  PREFIX-in.png       the page as it enters flat_field (already rectified)
  PREFIX-flat.png     after flat_field
  PREFIX-neutral.png  after neutralize_ink
  PREFIX-tone.png     after tone
  PREFIX-cmds.json    the magick commands those stages ran (bench/imbench.mjs)

  python3 bench/dump.py path/to/a4norm PREFIX -- [a4norm arguments]

It also prints how long ImageMagick took for each stage here.
"""
import importlib.machinery
import importlib.util
import json
import subprocess
import sys
import time

script, pre = sys.argv[1], sys.argv[2]
args = sys.argv[sys.argv.index("--") + 1:]
loader = importlib.machinery.SourceFileLoader("a4norm_oracle", script)
spec = importlib.util.spec_from_loader("a4norm_oracle", loader)
A = importlib.util.module_from_spec(spec)
loader.exec_module(A)

times, cmds, recording = {}, [], [False]
run = subprocess.run


def recorded(cmd, *a, **k):
    if recording[0] and isinstance(cmd, (list, tuple)) and str(cmd[0]).endswith("magick"):
        cmds.append([str(c) for c in cmd])
    return run(cmd, *a, **k)


A.subprocess.run = recorded


def kept(name, fn, save_in=False):
    def stage(job, *a, **k):
        if save_in:
            run(["magick", job.cur, f"PNG24:{pre}-in.png"], check=True)
        recording[0] = True
        t = time.time()
        r = fn(job, *a, **k)
        times[name] = time.time() - t
        recording[0] = False
        run(["magick", job.cur, f"PNG24:{pre}-{name}.png"], check=True)
        return r
    return stage


A.flat_field = kept("flat", A.flat_field, save_in=True)
A.neutralize_ink = kept("neutral", A.neutralize_ink)
A.tone = kept("tone", A.tone)
sys.argv = ["a4norm"] + args
A.main()
if not times:
    sys.exit("the page never reached flat_field (a photo, cards or a colour copy?)")
json.dump(cmds, open(pre + "-cmds.json", "w"), indent=1)
print("ImageMagick here: " + "  ".join(f"{k} {v:.3f}s" for k, v in times.items())
      + f"  sum {sum(times.values()):.3f}s")
