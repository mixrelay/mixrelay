# macOS packaging

ToneNari will use a signed and notarized installer package because the HAL
driver is installed under `/Library/Audio/Plug-Ins/HAL`.

Packaging must validate the driver file, ownership and mode, signature,
loaded UID/version, and end-to-end loopback independently. Install and
uninstall scripts must never recursively change permissions across the HAL
directory.
