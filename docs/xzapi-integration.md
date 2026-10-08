# xzapi H3 integration

Backend: `xzapi-video`

- API base URL: `https://api.xzapi.vip`
- Model: `ch1007-minimax-h3-2k`
- Output: 2K, 15 seconds, 16:9 or 9:16
- Protocol: https://docs.xzapi.vip/index.html
- Local references: PNG/JPEG, up to 20 MiB

The adapter uploads local references through the StarFrame upload-sign endpoint,
checks the uploaded bytes, submits `POST /v1/videos` once, polls the task, and
downloads its authenticated `/content` resource. It never retries paid creation.
Receipts are retained in `data/xzapi-tasks/`, including IDs on polling/download failure.

`data/gen-backends.json` contains non-secret settings. The API key and upload-sign
credential are stored in the existing Windows DPAPI keystore under `xzapi-video`
and `xzapi-upload-sign`. Do not copy credentials into source or logs.

The video composer supports a selected texture reference or an upstream image node.
The backend capabilities constrain resolution, duration, and aspect controls.

## Local network setup

This machine's global Clash/TUN route could not reach the StarFrame TOS bucket.
`data/xzapi-network.json` selects `WLAN` for that exact storage hostname only.
The adapter discovers the interface's current IPv4 address, resolves the public
storage IPs through DNS over HTTPS, and uses Windows curl with verified TLS.
Signed URLs are supplied on stdin rather than command-line arguments. Temporary
transfer files are removed after use. Global proxy and Windows security settings
are unchanged. If the active network interface changes, update `storageInterface`.
Remove this optional network file to use the ordinary storage route.

## Verification on 2026-09-09

- Live API key/model-list validation: passed.
- Live reference upload and byte-for-byte read-back: passed (13.54 seconds).
- xzapi tests: 3 passed, including the explicitly enabled live upload probe.
- Agentd backend registration test: passed.
- Client tests: 692 passed; client typecheck and production build passed.
- Agentd compilation: passed.
- Final service startup: blocked by Windows Code Integrity event 3077,
  policy `VerifiedAndReputableDesktop`, status `0xc0e90002`.
- Paid video generation: not submitted. No successful I2V output is claimed.

The first upload-only failure is preserved in
`data/xzapi-i2v-test-20260909.attempt.json` and
`data/xzapi-i2v-test-20260909.result.json`. Its request failed before video task
creation. Do not delete these records to retry. After the executable is trusted
through the system's approved signing/trust process, start `forge-agentd`, check
`/health`, and complete one real I2V submission with a separate attempt receipt.
