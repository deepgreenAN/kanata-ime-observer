# kanata ime observer

IME(Input Method Editor) aware layer switch for [kanata](https://github.com/jtroo/kanata).

## Support

kanata_ime_observer observes the following changes of IME.

| file suffix | framework    | target change             | example                              | short-cut key (example)         |
|-------------|--------------|---------------------------|--------------------------------------|---------------------------------|
| linux_ibus  | ibus         | input method engine       | "xkb:us::eng", "mozc-jp"             | `Super` + `Space`               |
| linux_fcitx | fcitx5       | input method              | "keyboard-jp", "mozc"                | `grave`, `ZenkakuHankaku`       |
| win_onoff   | IME(windows) | IME on, off               | "ime-on", "ime-off"                  | `grave`, `ZenkakuHankaku`       |
| win         | IME(windows) | keyboard layout           | "en-US", "ja-JP"                     | `Alt` + `Shift`, `Win` + `Space`|
| mac         | IME(macos)   | input source id           | "com.apple...RomajiTyping.Japanese"  | `ctl` + `Space`                 |

## Installation

you can download pre-build binaries from [release page](https://github.com/deepgreenAN/kanata-ime-observer/releases).

## Usage

If you run kanata like that,

```sh
kanata --cfg config.kbd --port 49500
```

you can configure `kanata_ime_observer layer`(suffix omitted) by specifying IME and layer name.

```sh
kanata_ime_observer layer 49500 --ime keyboard-jp --layer normal --ime mozc --layer oyayubi-shift
```

If you want to know IME names, run `kanata_ime_observer log`, which does not send any request to kanata.

```sh
kanata_ime_observer log 49500
```

If you want to switch config file instead of layer, you can use `kanata_ime_observer config`.

```sh
kanata --cfg normal.kbd --cfg oyayubi_shift.kbd --port 49500
kanata_ime_observer config 49500 --ime keyboard-jp --ime mozc
```

## Build

Build and run yourself.

```sh
git clone https://github.com/deepgreenAN/kanata-ime-observer
cd kanata-ime-observer
cargo build --release

target/release/kanata_ime_observer --help
```
