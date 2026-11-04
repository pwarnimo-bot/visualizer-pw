# visualizer-pw

Terminal audio visualizer for PipeWire / PulseAudio on Linux and WASAPI on Windows 10/11. It shows whatever your system is playing.

- **Spectrum analyzer**: log-spaced bars from 30 Hz to 16 kHz with falling peak markers
- **Oscilloscope**: braille waveform with auto-gain and trigger (overlay, split L/R, or goniometer/XY)
- **Amplifier meters**: analog VU needles with ballistics, and LED peak meters in dBFS with peak hold and a clip indicator
- **Phase correlation**: +1 is mono, 0 is wide or uncorrelated, below 0 (red) means the channels cancel when summed to mono

## Install (Arch)

```sh
cd packaging/arch
makepkg -si
```

## Build & run

```sh
cargo build --release
./target/release/visualizer-pw                 # monitor of the default output
./target/release/visualizer-pw -s <source>     # any source from --list
./target/release/visualizer-pw --list          # list capturable sources
```

Requires `libpulse` (provided by `pipewire-pulse` on Arch) and a truecolor terminal.

## Windows 10 / 11

Download `visualizer-pw.exe` from the [releases](../../releases) page (or the latest
CI run's artifacts) and run it from a terminal:

```powershell
.\visualizer-pw.exe                       # loopback of the default output device
.\visualizer-pw.exe --list                # list output devices
.\visualizer-pw.exe -s "Speakers (Realtek(R) Audio)"
```

It records what the output device is playing via WASAPI loopback, so no extra
drivers or "Stereo Mix" are needed, and it follows the default device when you
switch outputs. Use **Windows Terminal** (built into Windows 11, free from the
Microsoft Store on Windows 10). The legacy console renders colours and braille
poorly. The exe has no dependencies beyond Windows itself.

Build it yourself natively with `cargo build --release`, or cross-compile from Linux:

```sh
rustup target add x86_64-pc-windows-gnu    # plus mingw-w64-gcc
cargo build --release --target x86_64-pc-windows-gnu
```

## Keys

| Key | Action |
|---|---|
| `1` `2` `3` `4` / `Tab` | dashboard · spectrum · scope · meters |
| `+` `-` / `↑` `↓`, `0` | gain (spectrum & VU), reset |
| `,` `.` / `←` `→` | graph speed, ¼× to 4× (bars, meters, scope sweep) |
| `s` | number of spectrum bars |
| `m` | scope mode |
| `c` | colour theme (classic, amber, neon, red, a320) |
| `p` / `space` | freeze |
| `h` / `?` | help |
| `q` / `Esc` | quit |
