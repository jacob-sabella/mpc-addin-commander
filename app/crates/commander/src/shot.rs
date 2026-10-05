//! `--shot`: draws the app off screen and saves the frame as a PNG, with no window. It connects
//! as usual, selects an instance and page, waits for its skin, and renders the same egui frame
//! the window would, through wgpu into a texture. For comparing a panel with the device's screen.

use crate::model::{Shared, SkinState};
use crate::ui::{fonts, theme, App};
use anyhow::{anyhow, bail, Context};
use std::path::Path;
use std::time::{Duration, Instant};

/// What to shoot.
pub struct Shot {
    pub path: String,
    /// An instance name (the first whose name contains it, case-insensitively), or its position
    /// in the list from 0.
    pub select: Option<String>,
    pub page: usize,
    pub width: u32,
    pub height: u32,
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const WAIT: Duration = Duration::from_secs(30);

pub fn run(mut app: App, model: Shared, shot: Shot) -> anyhow::Result<()> {
    let id = wait_for(&model, shot.select.as_deref())?;
    if let Some(id) = id {
        app.select(id);
        app.set_page(id, shot.page);
    }
    pollster::block_on(render(&mut app, &shot))
}

/// Waits for the connection, the project and the chosen instance's skin; the chosen id.
fn wait_for(model: &Shared, select: Option<&str>) -> anyhow::Result<Option<u32>> {
    let start = Instant::now();
    loop {
        {
            let m = model.lock().unwrap();
            let all: Vec<_> = m.instances.iter().chain(&m.stock).collect();
            let ready = m.conn.is_connected() && m.project.is_some();
            let chosen = select.and_then(|s| match s.parse::<usize>() {
                Ok(n) => all.get(n).copied(),
                Err(_) => {
                    let s = s.to_lowercase();
                    all.iter()
                        .copied()
                        .find(|i| i.plugin.name.to_lowercase().contains(&s))
                }
            });
            if ready && (select.is_none() || chosen.is_some()) {
                let loading = chosen.is_some_and(|i| {
                    matches!(m.skins.get(&i.plugin.uid), Some(SkinState::Loading { .. }))
                });
                if !loading {
                    return Ok(chosen.map(|i| i.plugin.id));
                }
            }
        }
        if start.elapsed() > WAIT {
            bail!("--shot: nothing to shoot after {} s", WAIT.as_secs());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

async fn render(app: &mut App, shot: &Shot) -> anyhow::Result<()> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: if cfg!(target_os = "linux") {
            wgpu::Backends::VULKAN
        } else {
            wgpu::Backends::PRIMARY
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| anyhow!("no adapter: {e}"))?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .map_err(|e| anyhow!("request_device: {e}"))?;
    let (w, h) = (shot.width.max(64), shot.height.max(64));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shot"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut renderer = egui_wgpu::Renderer::new(
        &device,
        FORMAT,
        egui_wgpu::RendererOptions {
            msaa_samples: 1,
            depth_stencil_format: None,
            dithering: false,
            predictable_texture_filtering: false,
        },
    );
    let ctx = egui::Context::default();
    fonts::install(&ctx);
    theme::apply(&ctx);
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [w, h],
        pixels_per_point: 1.0,
    };
    // A few frames: the first lays out and uploads, later ones settle panels sized from content.
    for frame in 0..4 {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(w as f32, h as f32),
            )),
            max_texture_side: Some(device.limits().max_texture_dimension_2d as usize),
            time: Some(frame as f64 * 0.1),
            ..Default::default()
        };
        raw.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(1.0);
        let out = ctx.run_ui(raw, |ui| app.ui(ui));
        let jobs = ctx.tessellate(out.shapes, 1.0);
        let mut encoder = device.create_command_encoder(&Default::default());
        for (id, delta) in &out.textures_delta.set {
            renderer.update_texture(&device, &queue, *id, delta);
        }
        let extra = renderer.update_buffers(&device, &queue, &mut encoder, &jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shot"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(theme::CLEAR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        queue.submit(extra.into_iter().chain(std::iter::once(encoder.finish())));
        for id in &out.textures_delta.free {
            renderer.free_texture(id);
        }
    }
    save(&device, &queue, &texture, w, h, Path::new(&shot.path))
}

/// Copies the texture back and writes it as a PNG.
fn save(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    w: u32,
    h: u32,
    path: &Path,
) -> anyhow::Result<()> {
    let row =
        (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shot"),
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| anyhow!("poll: {e}"))?;
    rx.recv()?.map_err(|e| anyhow!("map: {e}"))?;
    let data = slice.get_mapped_range();
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        rgba.extend_from_slice(&data[y * row as usize..y * row as usize + w as usize * 4]);
    }
    drop(data);
    buffer.unmap();
    let img = image::RgbaImage::from_raw(w, h, rgba).ok_or_else(|| anyhow!("bad frame"))?;
    img.save(path)
        .with_context(|| format!("{}", path.display()))?;
    println!("commander --shot: wrote {} ({w}x{h})", path.display());
    Ok(())
}
