A simple (and useless) VNC server that just display a static image and
optinally set a cursor pointer.

Features:

- Custom background & pointer pictures
- Custom desktop name
- RFP (Remote Framebuffer Protocol) version 3.3, 3.7, and 3.8
- No authentication
- Pixel formats
    - True color (variable bit length)
    - Color map is NOT supported
- Picture encodings
    - Raw
    - ZRLE (Zlib Run-Length Encoding)

Known issues:

Color map is required by RFC but not implemented yet. This may cause
connection faliure on some clients and configuration. Set picture quality to
highest one on client in this case.