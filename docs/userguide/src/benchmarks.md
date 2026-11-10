# Benchmarks

<!-- Written by crates/aurafx-render/tests/benchmarks.rs; run it again to update. -->

How long common steps take, measured on one machine with the release build (the best of three runs; filters use all cores). Run them on your own machine with

```sh
cargo test --release -p aurafx-render --test benchmarks -- --ignored --nocapture
```

which rewrites this page.

Machine: Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz, 16 threads; GPU: AMD Radeon Graphics (RADV NAVI14) (Vulkan).

| Step | What | Time (ms) |
|---|---|---|
| Read | testdata/can.ex2 (Exodus II, 17.6 MB, the first time step) | 2.6 |
| Read | testdata/disk_out_ref.ex2 (Exodus II, 0.7 MB) | 2.4 |
| Source | Wavelet, 129³ points | 42.4 |
| Filter | Contour at one value, 129³ wavelet | 141.2 |
| Filter | Slice through the middle, 129³ wavelet | 108.6 |
| Filter | Clip by a plane, 129³ wavelet | 128.7 |
| Filter | Threshold (range 150 to 200), 129³ wavelet | 64.2 |
| Filter | Gradient of the scalar, 129³ wavelet | 258.8 |
| Filter | Resample To Image (64³), 129³ wavelet | 27.7 |
| Draw | Prepare the contour for drawing (824176 triangles) | 104.3 |
| Draw | A frame of the contour at 1280 x 720, 4x MSAA (115 frames per second) | 8.7 |
