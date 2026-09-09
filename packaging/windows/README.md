# Windows packaging

Windows needs two separately signed artifacts:

1. a WaveRT virtual audio driver that exposes the ToneNari render/capture endpoints;
2. the per-user Rust engine, which owns WASAPI process-loopback sessions and the portable graph.

The engine must run in the interactive user session, not Session 0. The development installer uses
an `ONLOGON` scheduled task for that reason. Driver installation requires elevation and `pnputil`;
production packages require Microsoft-compatible release signing. The app reports
`not_installed` until this driver boundary is available instead of pretending a silent endpoint is
healthy.

The implementation is based on the WaveRT architecture documented by Microsoft's SysVAD sample,
but ToneNari's transport and endpoint identity must remain product-owned. Do not ship the sample's
Contoso identities or its tone-generating capture path.
