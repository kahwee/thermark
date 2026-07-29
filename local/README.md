# Local prints (not in git)

Put personal stickers and real Wi‑Fi labels here. This directory is **gitignored**.

```bash
mkdir -p local/prints

# Prefer env for password (not shell history):
THERMARK_WIFI_PASSWORD='…' ./target/release/thermark wifi \
  --ssid "YourNetwork" --save local/prints/home-wifi.png --label 50x30

./target/release/thermark print -i local/prints/my-sticker.png --label 50x30
```

**Guards built into the CLI:**

- `wifi --save fixtures/…` is **rejected** (protects against committing secrets)
- Missing Wi‑Fi password → hints `THERMARK_WIFI_PASSWORD`
- JPEG print without `--dither` → tip for photo settings
- Print without `--label` on a large image → tip to use `50x30`
- BLE connect failures → tip to quit vendor apps

**Never** put real credentials under `fixtures/` — that path is for public product demos only.
