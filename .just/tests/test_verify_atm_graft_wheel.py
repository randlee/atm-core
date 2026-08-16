from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest
import zipfile


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from verify_atm_graft_wheel import verify_release_wheel


class VerifyAtmGraftWheelTests(unittest.TestCase):
    def write_wheel(self, directory: Path, filename: str, tags: list[str]) -> Path:
        wheel_path = directory / filename
        with zipfile.ZipFile(wheel_path, "w") as archive:
            archive.writestr(
                "atm_graft-1.4.2.dist-info/WHEEL",
                "Wheel-Version: 1.0\n" + "".join(f"Tag: {tag}\n" for tag in tags),
            )
        return wheel_path

    def test_accepts_composite_manylinux_filename_and_metadata_tags(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            wheel = self.write_wheel(
                Path(temporary_directory),
                "atm_graft-1.4.2-cp311-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
                ["cp311-abi3-manylinux_2_17_x86_64", "cp311-abi3-manylinux2014_x86_64"],
            )

            verify_release_wheel(wheel, "manylinux_2_17_x86_64.manylinux2014_x86_64")

    def test_rejects_wrong_platform_filename_tag(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            wheel = self.write_wheel(
                Path(temporary_directory),
                "atm_graft-1.4.2-cp311-abi3-win_amd64.whl",
                ["cp311-abi3-win_amd64"],
            )

            with self.assertRaisesRegex(RuntimeError, "manylinux_2_17_x86_64"):
                verify_release_wheel(wheel, "manylinux_2_17_x86_64")

    def test_rejects_missing_platform_metadata_tag(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            wheel = self.write_wheel(
                Path(temporary_directory),
                "atm_graft-1.4.2-cp311-abi3-musllinux_1_2_x86_64.whl",
                ["cp311-abi3-manylinux_2_17_x86_64"],
            )

            with self.assertRaisesRegex(RuntimeError, "musllinux_1_2_x86_64"):
                verify_release_wheel(wheel, "musllinux_1_2_x86_64")


if __name__ == "__main__":
    unittest.main()
