//! Rasterise the console offscreen and write a PNG.
//!
//! `cargo run -p gui --example render -- /tmp/console.png`
//!
//! There is no display on the machine this was built on, and the cost of
//! that was a console that did not look like the design it was drawn from.
//! wgpu renders surfaceless, and Mesa ships lavapipe, so "no display" was
//! never the same thing as "cannot see it".

use eframe::egui;

const WIDTH: u32 = 1600;
const HEIGHT: u32 = 1000;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "console.png".to_string());

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("no wgpu adapter -- is a Vulkan driver (lavapipe) installed?");
    eprintln!("adapter: {:?}", adapter.get_info());

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("offscreen"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: Default::default(),
        },
        None,
    ))
    .expect("request_device");

    // Drive the real console for one frame.
    let ctx = egui::Context::default();
    gui::theme::install(&ctx);
    let mut console = gui::app::Console::new("shack.local:4532".to_string());
    console.demo_capabilities(gui::demo::ts570d());
    console.demo_state();
    // A real band, from the same generator the emulator serves, so the
    // still shows the waterfall under something like live conditions.
    let band = cat_signal::synthetic::Band::populated(14_000_000, 14_350_000, 43, -110.0, 7);
    let frames: Vec<_> = (0..220)
        .map(|i| band.frame(14_074_000, 48_000, 1024, f64::from(i) * 0.08, i as u64))
        .collect();
    console.demo_spectrum(&frames);

    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(WIDTH as f32, HEIGHT as f32),
        )),
        ..Default::default()
    };
    // Twice: the first pass lets egui size things it lays out from
    // previous-frame geometry, so a one-pass capture misreports widths.
    // Both frames' texture deltas are kept. The font atlas is created
    // during the FIRST frame and its delta arrives with that frame's
    // output -- and egui's *solid* shapes sample a white texel inside that
    // atlas, so discarding the first delta does not merely lose text, it
    // makes every rectangle sample a texture that was never uploaded and
    // the whole frame renders as nothing at all.
    let first = ctx.run(raw.clone(), |ctx| console.draw(ctx));
    let output = ctx.run(raw, |ctx| console.draw(ctx));
    let deltas: Vec<_> = first
        .textures_delta
        .set
        .into_iter()
        .chain(output.textures_delta.set.clone())
        .collect();

    let pixels_per_point = ctx.pixels_per_point();
    eprintln!("shapes: {}", output.shapes.len());
    let jobs = ctx.tessellate(output.shapes, pixels_per_point);
    let verts: usize = jobs
        .iter()
        .map(|j| match &j.primitive {
            egui::epaint::Primitive::Mesh(m) => m.vertices.len(),
            _ => 0,
        })
        .sum();
    eprintln!(
        "paint jobs: {}  vertices: {}  ppp: {}",
        jobs.len(),
        verts,
        pixels_per_point
    );

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);
    for (id, delta) in &deltas {
        renderer.update_texture(&device, &queue, *id, delta);
    }
    eprintln!("textures uploaded: {}", deltas.len());

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());

    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [WIDTH, HEIGHT],
        pixels_per_point,
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.update_buffers(&device, &queue, &mut encoder, &jobs, &screen);
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.023,
                        g: 0.031,
                        b: 0.043,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
    }

    // Read back. Rows are padded to 256 bytes, which is easy to forget and
    // produces a sheared image rather than an error.
    let bytes_per_row = (WIDTH * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(bytes_per_row) * u64::from(HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range();

    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for row in 0..HEIGHT {
        let start = (row * bytes_per_row) as usize;
        pixels.extend_from_slice(&data[start..start + (WIDTH * 4) as usize]);
    }
    image::save_buffer(&out, &pixels, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("write png");
    eprintln!("wrote {out}");
}
