# Local prints (not in git)

Put personal stickers and real Wi‑Fi labels here. This directory is **gitignored**.

```bash
mkdir -p local/prints
# example:
./target/release/thermark print -i local/prints/my-sticker.png --label 50x30
./target/release/thermark wifi --ssid "…" --password '…' --save local/prints/home-wifi.png
```

**Never** put real credentials under `fixtures/` — that path is for public product demos only.
