# _tid_ &mdash; a small system information bar

![screenshot of tid in action](example.png)

This little program is _slightly_ under construction. 
It is intended for my personal use, but by now it is getting polished enough that it may be useful to others as well.
Further improvements will come, especially based on problems from users beyond myself.
I am enjoying it a lot and it fits my needs.

(See also, accompanying [labbook entry](https://dwangschematiek.nl/labbook/tid/).)

## installation

```
git clone https://git.sr.ht/~ma3ke/tid
cd tid
cargo install --path .
sudo mkdir /etc/tid
sudo cp -r fonts /etc/tid/fonts
```

(I may put these operations into a makefile or shell script at some point, but it's quite trivial.)

## usage & configuration

As of now, _tid_ can be configured through command line arguments.

- **Want to change the font?** 
  Fonts can be specified through command line arguments. Use, for example, the `--font-name geneva12.uf2` or `--font-path /etc/tid/fonts/geneva12.uf2`.
  Currently, the supported font formats are [uf2](https://wiki.xxiivv.com/site/ufx_format.html) and [psf2](https://en.wikipedia.org/wiki/PC_Screen_Font). 
  For instance, [here](https://hachyderm.io/@ma3ke/111376077963594124) you can see _tid_ running with the beautiful `Sun12x22.psfu` font.
  Note that uf2 fonts require a `.uf2` extension to be accepted, while `psf2` can be recognized through its magic number.
- **Want to change the color?** 
  You can set the foreground and background color by providing a `0x{r}{g}{b}{a}` formatted hex string as parameters after the `--fg` and `--bg` flags, respectively.
  For example, 

  ```
  tid --fg 0xcc33aaff --bg 0xffffff00
  ```

  will set the foreground to a dark magenta and the background to white transparent, like [this](https://hachyderm.io/@ma3ke/111377402365783978).
  By default, the background is black and transparent (if supported), and the foreground white.

### full usage information

```
Usage:
    tid [OPTIONS]

Options:
    --font-name -n    Set the font name from the default directory.
                      (default: 'cream12.uf2' in '/etc/tid/fonts')
    --font-path -p    Set the font path.
    --fg              Specify the foreground color as an rgba hex string.
                      (default: 0xffffffff)
    --bg              Specify the background color as an rgba hex string.
                      (default: 0x00000000)
    --version   -v    Display function.
    --help      -h    Display help.
```

## weirdness

The background should be transparent, but it may not be depending on your window manager.
For example, my installation of [_hikari_](https://hikari.acmelabs.space/) does not support transparency.
But on _bspwm_ (X11), the transparency works fine.

---

Thanks &lt;3 [ma3ke](https://dwangschematiek.nl)
