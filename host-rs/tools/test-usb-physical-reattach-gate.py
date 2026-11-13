#!/usr/bin/env python3
"""Synthetic acceptance checks for the human-operated Linux physical USB gate.

Never asserts real hardware success; uses a disposable fake sysfs and fake host.
"""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent


def exercise(mode: str):
    with tempfile.TemporaryDirectory() as td:
        work = Path(td)
        sysfs = work / "sysfs"
        sysfs.mkdir()
        port = sysfs / "1-2.3"

        def attach():
            port.mkdir(exist_ok=True)
            (port / "idVendor").write_text("0fce\n")
            (port / "idProduct").write_text("020d\n")

        attach()
        count = work / "counter"
        host = work / "fake-host"
        host.write_text(
            "#!/usr/bin/env bash\n"
            "set -euo pipefail\n"
            "[[ \"$*\" == '--usb-probe --frames 1000 --rate 32 --budget-ms 2000 --device 0fce:020d' ]]\n"
            "n=0; [[ ! -f \"$FAKE_COUNTER\" ]] || n=\"$(cat \"$FAKE_COUNTER\")\"\n"
            "n=$((n+1)); printf '%s\\n' \"$n\" > \"$FAKE_COUNTER\"\n"
            "echo 'sent 1000 echoed 1000 sequence-mismatch 0 payload-mismatch 0 timed-out 0 skipped 1'\n"
            "if [[ \"${FAKE_FAIL_AT:-}\" == \"$n\" ]]; then echo FAIL; exit 1; fi\n"
            "echo PASS\n"
        )
        host.chmod(0o700)
        env = dict(os.environ, MWB_USB_SYSFS_ROOT=str(sysfs), FAKE_COUNTER=str(count))
        if mode == "failed_probe":
            env["FAKE_FAIL_AT"] = "2"
        cmd = ["bash", str(HERE / "usb-physical-reattach-gate.sh"), "--device", "0fce:020d", "--runs", "2", "--bin", str(host), "--out", str(work)]
        proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1, env=env)
        output = []
        try:
            for line in proc.stdout:
                output.append(line)
                if line.startswith("UNPLUG "):
                    if mode != "no_detach":
                        shutil.rmtree(port)
                    proc.stdin.write("\n")
                    proc.stdin.flush()
                elif line.startswith("REPLUG "):
                    if mode != "no_reattach":
                        attach()
                    proc.stdin.write("\n")
                    proc.stdin.flush()
            code = proc.wait(timeout=10)
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait()
        evidence = list(work.glob("mwb-usb-physical.*/summary.tsv"))
        assert len(evidence) == 1, (mode, output)
        summary = evidence[0].read_text()
        metadata = evidence[0].with_name("metadata.txt").read_text()
        assert "evidence_mode=SYNTHETIC" in metadata
        assert "usb_port=1-2.3" in metadata
        if mode == "success":
            assert code == 0, (code, output, summary)
            assert summary.count("\tPASS\n") == 2, summary
            assert count.read_text().strip() == "2"
        elif mode == "failed_probe":
            assert code == 1 and summary.count("\tPASS\n") == 1 and summary.count("\tFAIL\n") == 1
        elif mode == "no_detach":
            assert code == 1 and "\tNO\tNO\tNA\tNA\tFAIL\n" in summary
            assert not count.exists(), "no USB probe should run without verified detach"
        elif mode == "no_reattach":
            assert code == 1 and "\tYES\tNO\tNA\tNA\tFAIL\n" in summary
            assert not count.exists(), "no USB probe should run without verified reattach"
        assert evidence[0].stat().st_mode & 0o077 == 0, "summary must not expose hardware evidence"


for scenario in ("success", "failed_probe", "no_detach", "no_reattach"):
    exercise(scenario)
print("PASS simulated physical reattach gate: success/failure/missing-detach/missing-reattach")
