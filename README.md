# visualizer-pw

Terminal audio visualizer for PipeWire / PulseAudio. It shows whatever your system is playing.

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
./target/release/visualizer-pw -s <source>     # any source from `pactl list short sources`
```

Requires `libpulse` (provided by `pipewire-pulse` on Arch) and a truecolor terminal.

## Keys

| Key | Action |
|---|---|
| `1` `2` `3` `4` / `Tab` | dashboard · spectrum · scope · meters |
| `+` `-` / `↑` `↓`, `0` | gain (spectrum & VU), reset |
| `s` | number of spectrum bars |
| `m` | scope mode |
| `c` | colour theme (classic, amber, neon) |
| `p` / `space` | freeze |
| `h` / `?` | help |
| `q` / `Esc` | quit |
