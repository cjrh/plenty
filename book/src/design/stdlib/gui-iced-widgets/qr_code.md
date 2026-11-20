# QRCode

![Ticket text and its scannable QR code](screenshots/qr_code.png)

Run `plenty run --bin qr_code` in `examples/gui/iced_widgets`. Scanning the code yields the text **Plenty ticket 42**. The widget displays data; it does not scan codes or open links.

Pass nonempty UTF-8 text to `frame.qr_code`. The encoder chooses the code version automatically. The payload counts toward the 4,096-byte widget and 65,536-byte frame budgets. Encoder capacity depends on the content; text can return `LimitExceeded` even below 4,096 bytes. The binding uses four logical pixels per module and does not expose error-correction or styling options.

```plenty build
{{#include ../../../../../examples/gui/iced_widgets/src/qr_code.plenty}}
```

## Acknowledgements

- [qrcode-rust](https://docs.rs/qrcode/0.13.0/qrcode/) encodes the symbol for iced (MIT OR Apache-2.0).
