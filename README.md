# _tid_ &mdash; a small system information bar

![screenshot of tid in action](example.png)

This little program is _very_ under construction. 
It is intended for my personal use, so it is not polished to suit usability for others, at this moment.
This may change, at some point.
But right now, it works for me and it makes me happy.

(See also, accompanying [labbook entry](https://dwangschematiek.nl/labbook/tid/).)

## installation

```
git clone https://git.sr.ht/~ma3ke/tid
cd tid
cargo install --path .
sudo mkdir /etc/fonts
sudo cp -r fonts /etc/tid/fonts
```

## weirdness

The background should be transparent, but it may not be depending on your window manager.
For example, my installation of [_hikari_](https://hikari.acmelabs.space/) does not support transparency.
But on _bspwm_ (X11), the transparency works fine.

## configuration

_Some_ beyond editing the source.

- **Want to change the font?** Can be specified through command line arguments. Use, for example, the `--font-name geneva12.uf2` or `--font-path /etc/tid/fonts/geneva12.uf2`.
- **Want to change the color?** Futz around with the `BACKGROUND` and `FOREGROUND` constants.
The color format is `[u8; 4]` ordered as `[red, green, blue, alpha]`.

---

Thanks &lt;3 [ma3ke](https://dwangschematiek.nl)
