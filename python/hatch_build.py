import os
import shutil
import subprocess
import sys
from pathlib import Path

from hatchling.builders.hooks.plugin.interface import BuildHookInterface


class CustomBuildHook(BuildHookInterface):
    def initialize(self, version, build_data):
        """
        Build the Rust FFI library and copy it to the package directory.
        """
        # Get paths - handle both dev and sdist scenarios
        build_dir = Path(__file__).parent
        package_dir = build_dir / "pdf_strings"

        # Check if we're in an sdist or dev environment
        # In sdist, the Rust code will be in the same directory
        cargo_toml = build_dir / "Cargo.toml"
        if not cargo_toml.exists():
            # In dev mode, go up one level
            cargo_toml = build_dir.parent / "Cargo.toml"

        project_root = cargo_toml.parent

        # Determine library name based on platform
        if sys.platform == "darwin":
            lib_name = "libpdf_strings_ffi.dylib"
        elif sys.platform == "win32":
            lib_name = "pdf_strings_ffi.dll"
        else:
            lib_name = "libpdf_strings_ffi.so"

        print(f"Building Rust FFI library for {sys.platform}...")

        # Build the Rust library
        subprocess.run(
            ["cargo", "build", "-p", "pdf-strings-ffi", "--release"],
            cwd=project_root,
            check=True,
        )

        # Copy the built library to the package directory
        target_lib = project_root / "target" / "release" / lib_name
        dest_lib = package_dir / lib_name

        print(f"Copying {target_lib} to {dest_lib}")
        shutil.copy2(target_lib, dest_lib)

        # Mark the wheel as platform-specific but Python-version-independent
        # Since we use ctypes (pure Python), only the shared library is platform-specific
        # Set tag to py3-none-{platform} instead of cp{ver}-cp{ver}-{platform}
        import platform as plat

        # Build a reasonable platform tag
        if sys.platform == "darwin":
            machine = plat.machine().lower()
            # ARM64 Macs only exist from macOS 11.0 (Big Sur) onwards
            # x86_64 Macs can use 10.12 as minimum
            min_macos = "11_0" if machine == "arm64" else "10_12"
            platform_tag = f"macosx_{min_macos}_{machine}"
        elif sys.platform == "win32":
            machine = plat.machine().lower()
            platform_tag = f"win_{machine}"
        else:
            # Linux: use manylinux_2_17 (compatible with manylinux2014)
            machine = plat.machine()
            platform_tag = f"manylinux_2_17_{machine}"

        # Set explicit tag: py3 (all Python 3), none (no ABI), platform-specific
        build_data['tag'] = f'py3-none-{platform_tag}'

        print("Rust FFI library built and copied successfully")
