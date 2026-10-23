# Supported qualification environments

Generated from `support/environments.json`. A listed target is a release requirement, not a passing result.

| Environment | OS / distribution | Version rule | CPU | Same candidate artifact |
| --- | --- | --- | --- | --- |
| macos-14-2-arm64 | macos | 14.2 (minor) | arm64 | macos-universal-pkg |
| macos-14-2-x64 | macos | 14.2 (minor) | x64 | macos-universal-pkg |
| macos-15-arm64 | macos | 15 (major) | arm64 | macos-universal-pkg |
| macos-15-x64 | macos | 15 (major) | x64 | macos-universal-pkg |
| macos-26-arm64 | macos | 26 (major) | arm64 | macos-universal-pkg |
| macos-26-x64 | macos | 26 (major) | x64 | macos-universal-pkg |
| macos-27-arm64 | macos | 27 (major) | arm64 | macos-universal-pkg |
| macos-27-x64 | macos | 27 (major) | x64 | macos-universal-pkg |
| debian-13-x64 | debian | 13 (major) | x64 | linux-x64-deb |
| debian-13-arm64 | debian | 13 (major) | arm64 | linux-arm64-deb |
| ubuntu-26-04-x64 | ubuntu | 26.04 (minor) | x64 | linux-x64-deb |
| ubuntu-26-04-arm64 | ubuntu | 26.04 (minor) | arm64 | linux-arm64-deb |
| kali-rolling-x64 | kali | rolling (rolling) | x64 | linux-x64-deb |
| kali-rolling-arm64 | kali | rolling (rolling) | arm64 | linux-arm64-deb |
| arch-rolling-x64 | arch | rolling (rolling) | x64 | linux-x64-arch |

Rolling targets require the exact OS release, kernel, dependency versions and package-inventory hash in each qualification report.
