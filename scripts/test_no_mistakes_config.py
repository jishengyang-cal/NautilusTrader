#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Exercise the no-mistakes prepare command without building the workspace.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parents[1]
CONFIG_PATH = REPO_ROOT / ".no-mistakes.yaml"
EXPECTED_RETRY_CALLS = 2


def write_executable(path: Path, content: str) -> None:
    """
    Write an executable probe script.
    """
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)


def run_prepare(
    command: str,
    bin_dir: Path,
    output_path: Path,
    home_dir: Path,
    overrides: dict[str, str],
) -> list[str]:
    """
    Run the repository-owned prepare command in an isolated probe environment.
    """
    env = os.environ.copy()
    real_make = shutil.which("make", path=env["PATH"]) or "make"
    for name in (
        "CARGO_BUILD_JOBS",
        "CARGO_CI_PROFILE",
        "CARGO_TARGET_DIR",
        "CARGO_TARGET_ROOT",
    ):
        env.pop(name, None)
    env.update(overrides)
    env.update(
        {
            "HOME": str(home_dir),
            "PATH": f"{bin_dir}{os.pathsep}{env['PATH']}",
            "PREPARE_PROBE_OUTPUT": str(output_path),
            "REAL_MAKE": real_make,
        },
    )
    subprocess.run(
        ["/bin/sh", "-c", command],
        cwd=REPO_ROOT,
        env=env,
        check=True,
    )
    return output_path.read_text(encoding="utf-8").splitlines()


def assert_prepare_case(
    command: str,
    bin_dir: Path,
    output_path: Path,
    home_dir: Path,
    overrides: dict[str, str],
    expected_target: Path,
) -> None:
    """
    Assert one prepare path-selection case and its published metadata.
    """
    observed = run_prepare(command, bin_dir, output_path, home_dir, overrides)
    call_prefix = f"uv_call={REPO_ROOT / 'python'}\t2\t{expected_target}\t"
    expected = [
        "make_args=build-debug",
        f"{call_prefix}run --no-sync python generate_stubs.py",
        f"{call_prefix}run --no-sync maturin develop --profile nextest",
    ]
    if observed != expected:
        raise AssertionError(
            f"prepare environment mismatch: {observed!r} != {expected!r}",
        )
    for metadata_name in (".py-stubs.inputs",):
        if not (expected_target / metadata_name).is_file():
            raise AssertionError(
                f"missing target metadata: {expected_target / metadata_name}",
            )


def create_probe_commands(bin_dir: Path) -> None:
    """
    Create the isolated command probes used by the regression tests.
    """
    write_executable(
        bin_dir / "rtk",
        """#!/bin/sh
if [ "$1" != "proxy" ]; then
    exit 64
fi
shift
exec "$@"
""",
    )
    write_executable(
        bin_dir / "make",
        """#!/bin/sh
printf 'make_args=%s\\n' "$*" > "$PREPARE_PROBE_OUTPUT"
exec "$REAL_MAKE" --no-print-directory \
    --old-file=check-cargo-cooldown \
    --old-file=sync \
    "$@"
""",
    )
    write_executable(
        bin_dir / "uv",
        """#!/bin/sh
{
    printf 'uv_call=%s\\t%s\\t%s\\t%s\\n' \
        "$PWD" "$CARGO_BUILD_JOBS" "$CARGO_TARGET_DIR" "$*"
} >> "$PREPARE_PROBE_OUTPUT"
if [ "$*" = "run --no-sync python generate_stubs.py" ] && \
    [ -n "$FAIL_STUB_ONCE_MARKER" ] && [ ! -e "$FAIL_STUB_ONCE_MARKER" ]; then
    touch "$FAIL_STUB_ONCE_MARKER"
    exit 1
fi
""",
    )


def check_path_selection(
    command: str,
    bin_dir: Path,
    output_path: Path,
    home_dir: Path,
    temp_dir: Path,
) -> None:
    """
    Check environment-driven persistent cache path selection.
    """
    shell_marker = temp_dir / "make-shell-ran"
    relative_root = Path(os.path.relpath(temp_dir / "relative", REPO_ROOT))
    cases = (
        (
            {
                "CARGO_BUILD_JOBS": "8",
                "CARGO_TARGET_DIR": str(relative_root / "target cache"),
                "CARGO_TARGET_ROOT": "/ignored-root",
            },
            REPO_ROOT / relative_root / "target cache",
        ),
        (
            {"CARGO_TARGET_DIR": str(temp_dir / "absolute target")},
            temp_dir / "absolute target",
        ),
        (
            {"CARGO_TARGET_ROOT": str(relative_root / "cargo root")},
            REPO_ROOT / relative_root / "cargo root" / "nautilus",
        ),
        (
            {"CARGO_TARGET_DIR": str(relative_root / "cache $dollar")},
            REPO_ROOT / relative_root / "cache $dollar",
        ),
        (
            {
                "CARGO_TARGET_DIR": (
                    f"{relative_root}/cache $(shell touch {shell_marker}) expression"
                ),
            },
            REPO_ROOT / relative_root / f"cache $(shell touch {shell_marker}) expression",
        ),
        ({}, home_dir / ".cache" / "nautilus-no-mistakes-target"),
    )
    for overrides, expected_target in cases:
        assert_prepare_case(
            command,
            bin_dir,
            output_path,
            home_dir,
            overrides,
            expected_target,
        )
    if shell_marker.exists():
        raise AssertionError("Make expanded the cache path as an expression")


def check_explicit_target(
    bin_dir: Path,
    output_path: Path,
    temp_dir: Path,
) -> None:
    """
    Check a command-line TARGET_DIR override.
    """
    explicit_target = temp_dir / "explicit $target cache"
    output_path.write_text("make_args=build-debug\n", encoding="utf-8")
    env = os.environ.copy()
    real_make = shutil.which("make", path=env["PATH"]) or "make"
    env.update(
        {
            "CARGO_BUILD_JOBS": "2",
            "PATH": f"{bin_dir}{os.pathsep}{env['PATH']}",
            "PREPARE_PROBE_OUTPUT": str(output_path),
        },
    )
    subprocess.run(
        [
            real_make,
            "--no-print-directory",
            "--old-file=check-cargo-cooldown",
            "--old-file=sync",
            "build-debug",
            f"TARGET_DIR={explicit_target}",
        ],
        cwd=REPO_ROOT,
        env=env,
        check=True,
    )
    call_prefix = f"uv_call={REPO_ROOT / 'python'}\t2\t{explicit_target}\t"
    expected = [
        "make_args=build-debug",
        f"{call_prefix}run --no-sync python generate_stubs.py",
        f"{call_prefix}run --no-sync maturin develop --profile nextest",
    ]
    observed = output_path.read_text(encoding="utf-8").splitlines()
    if observed != expected:
        raise AssertionError(f"TARGET_DIR mismatch: {observed!r} != {expected!r}")
    for metadata_name in (".py-stubs.inputs",):
        if not (explicit_target / metadata_name).is_file():
            raise AssertionError(
                f"missing TARGET_DIR metadata: {explicit_target / metadata_name}",
            )


def check_failed_generation_retry(bin_dir: Path, temp_dir: Path) -> None:
    """
    Check that failed stub generation preserves and then retries metadata.
    """
    retry_target = temp_dir / "failure retry cache"
    retry_target.mkdir()
    retry_input_list = retry_target / ".py-stubs.inputs"
    stale_input_list = "deleted-input\n"
    retry_input_list.write_text(stale_input_list, encoding="utf-8")
    failure_marker = temp_dir / "stub-failed-once"
    retry_output = temp_dir / "retry-output"
    retry_env = os.environ.copy()
    real_make = shutil.which("make", path=retry_env["PATH"]) or "make"
    retry_env.update(
        {
            "CARGO_TARGET_DIR": str(retry_target),
            "FAIL_STUB_ONCE_MARKER": str(failure_marker),
            "PATH": f"{bin_dir}{os.pathsep}{retry_env['PATH']}",
            "PREPARE_PROBE_OUTPUT": str(retry_output),
        },
    )
    retry_command = [
        real_make,
        "--no-print-directory",
        "--old-file=check-cargo-cooldown",
        "--old-file=sync",
        "py-stubs",
    ]
    failed = subprocess.run(retry_command, cwd=REPO_ROOT, env=retry_env, check=False)
    if failed.returncode == 0:
        raise AssertionError("stub generation failure unexpectedly succeeded")
    if retry_input_list.read_text(encoding="utf-8") != stale_input_list:
        raise AssertionError("failed generation published the new input list")
    subprocess.run(retry_command, cwd=REPO_ROOT, env=retry_env, check=True)
    retry_calls = retry_output.read_text(encoding="utf-8").splitlines()
    if len(retry_calls) != EXPECTED_RETRY_CALLS:
        raise AssertionError(f"stub generation was not retried: {retry_calls!r}")
    if retry_input_list.read_text(encoding="utf-8") == stale_input_list:
        raise AssertionError("successful retry did not publish the new input list")


def check_older_content_regenerates(bin_dir: Path, temp_dir: Path) -> None:
    """
    Check that changed content regenerates even when its timestamp is older.
    """
    content_target = temp_dir / "content cache"
    tracked_input = temp_dir / "tracked-input"
    content_output = temp_dir / "content-output"
    content_env = os.environ.copy()
    real_make = shutil.which("make", path=content_env["PATH"]) or "make"
    content_env.update(
        {
            "CARGO_TARGET_DIR": str(content_target),
            "PATH": f"{bin_dir}{os.pathsep}{content_env['PATH']}",
            "PREPARE_PROBE_OUTPUT": str(content_output),
        },
    )
    content_command = [
        real_make,
        "--no-print-directory",
        "--old-file=check-cargo-cooldown",
        "--old-file=sync",
        "py-stubs",
        f"PY_STUB_INPUT_LIST_COMMAND=printf '%s\\n' '{tracked_input}'",
    ]
    tracked_input.write_text("first worktree\n", encoding="utf-8")
    subprocess.run(content_command, cwd=REPO_ROOT, env=content_env, check=True)
    legacy_stamp = content_target / ".py-stubs.stamp"
    old_mtime = legacy_stamp.stat().st_mtime - 3600 if legacy_stamp.exists() else 1
    tracked_input.write_text("older second worktree\n", encoding="utf-8")
    os.utime(tracked_input, (old_mtime, old_mtime))
    subprocess.run(content_command, cwd=REPO_ROOT, env=content_env, check=True)
    content_calls = content_output.read_text(encoding="utf-8").splitlines()
    if len(content_calls) != EXPECTED_RETRY_CALLS:
        raise AssertionError(f"older changed content was not regenerated: {content_calls!r}")


def main() -> None:
    """
    Run cache-selection and failure-retry regressions.
    """
    config = yaml.safe_load(CONFIG_PATH.read_text(encoding="utf-8"))
    command = config["commands"]["prepare"]

    with tempfile.TemporaryDirectory(prefix="nautilus-no-mistakes-") as temp:
        temp_dir = Path(temp)
        bin_dir = temp_dir / "bin"
        home_dir = temp_dir / "home"
        output_path = temp_dir / "prepare-output"
        bin_dir.mkdir()
        home_dir.mkdir()
        create_probe_commands(bin_dir)
        check_path_selection(command, bin_dir, output_path, home_dir, temp_dir)
        check_explicit_target(bin_dir, output_path, temp_dir)
        check_failed_generation_retry(bin_dir, temp_dir)
        check_older_content_regenerates(bin_dir, temp_dir)


if __name__ == "__main__":
    main()
