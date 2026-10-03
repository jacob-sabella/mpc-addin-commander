//! The window: winit for the event loop and input, wgpu for the surface, egui-wgpu to draw
//! the UI, repainting continuously at the display's rate.

use crate::ui::{fonts, theme, App};
use anyhow::{anyhow, Context};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

pub fn run(app: App) -> anyhow::Result<()> {
    let event_loop = EventLoop::new().context("event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let egui = egui::Context::default();
    fonts::install(&egui);
    theme::apply(&egui);
    let mut shell = Shell {
        app,
        egui,
        window: None,
        gpu: None,
        renderer: None,
        input: Input::default(),
        start: Instant::now(),
    };
    event_loop.run_app(&mut shell).context("event loop")?;
    Ok(())
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

impl Gpu {
    async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        // Vulkan only on Linux: the GL probe is unstable on some drivers.
        let backends = if cfg!(target_os = "linux") {
            wgpu::Backends::VULKAN
        } else {
            wgpu::Backends::PRIMARY
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| anyhow!("create_surface: {e}"))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| anyhow!("no compatible adapter: {e}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("commander"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| anyhow!("request_device: {e}"))?;
        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        // egui does its own gamma handling and wants a non-sRGB framebuffer.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        Ok(Gpu {
            surface,
            device,
            queue,
            config,
        })
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.config.width = w.max(1);
        self.config.height = h.max(1);
        self.surface.configure(&self.device, &self.config);
    }
}

struct Shell {
    app: App,
    egui: egui::Context,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    renderer: Option<egui_wgpu::Renderer>,
    input: Input,
    start: Instant,
}

impl Shell {
    fn pixels_per_point(&self) -> f32 {
        self.window
            .as_ref()
            .map_or(1.0, |w| w.scale_factor() as f32)
    }

    fn frame(&mut self) {
        let (Some(window), Some(gpu), Some(renderer)) =
            (&self.window, &mut self.gpu, &mut self.renderer)
        else {
            return;
        };
        let ppp = window.scale_factor() as f32;
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let raw = self.input.take(
            size.width,
            size.height,
            ppp,
            self.start.elapsed().as_secs_f64(),
        );
        let out = self.egui.run_ui(raw, |ui| self.app.ui(ui));
        let ppp = self.egui.pixels_per_point();
        let jobs = self.egui.tessellate(out.shapes, ppp);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point: ppp,
        };
        use wgpu::CurrentSurfaceTexture as Cur;
        let frame = match gpu.surface.get_current_texture() {
            Cur::Success(f) | Cur::Suboptimal(f) => f,
            Cur::Lost | Cur::Outdated => {
                gpu.resize(size.width, size.height);
                return;
            }
            Cur::Timeout | Cur::Occluded => return,
            Cur::Validation => {
                log::error!("surface: validation error");
                std::process::exit(1);
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("egui"),
            });
        for (id, delta) in &out.textures_delta.set {
            renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
        }
        let extra = renderer.update_buffers(&gpu.device, &gpu.queue, &mut encoder, &jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
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
        gpu.queue
            .submit(extra.into_iter().chain(std::iter::once(encoder.finish())));
        frame.present();
        for id in &out.textures_delta.free {
            renderer.free_texture(id);
        }
    }
}

impl ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let cfg = &self.app.config.window;
        let attrs = Window::default_attributes()
            .with_title("Commander")
            .with_inner_size(winit::dpi::LogicalSize::new(
                cfg.width.max(640.0),
                cfg.height.max(400.0),
            ));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("window: {e}");
                event_loop.exit();
                return;
            }
        };
        let gpu = match pollster::block_on(Gpu::new(window.clone())) {
            Ok(g) => g,
            Err(e) => {
                log::error!("GPU: {e:#}");
                event_loop.exit();
                return;
            }
        };
        self.renderer = Some(egui_wgpu::Renderer::new(
            &gpu.device,
            gpu.config.format,
            egui_wgpu::RendererOptions {
                msaa_samples: 1,
                depth_stencil_format: None,
                dithering: true,
                predictable_texture_filtering: false,
            },
        ));
        self.window = Some(window);
        self.gpu = Some(gpu);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(w) = &self.window {
                    let ppp = w.scale_factor() as f32;
                    let s = w.inner_size();
                    self.app
                        .save_window(s.width as f32 / ppp, s.height as f32 / ppp);
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => self.frame(),
            other => {
                let ppp = self.pixels_per_point();
                self.input.on_event(&other, ppp);
            }
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

/// Translates winit window events into egui's input for the next frame.
#[derive(Default)]
struct Input {
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    pointer: Option<egui::Pos2>,
    focused: bool,
}

impl Input {
    fn on_event(&mut self, event: &WindowEvent, ppp: f32) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let pos = egui::pos2(position.x as f32 / ppp, position.y as f32 / ppp);
                self.pointer = Some(pos);
                self.events.push(egui::Event::PointerMoved(pos));
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
                self.events.push(egui::Event::PointerGone);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => egui::PointerButton::Primary,
                    MouseButton::Right => egui::PointerButton::Secondary,
                    MouseButton::Middle => egui::PointerButton::Middle,
                    _ => return,
                };
                if let Some(pos) = self.pointer {
                    self.events.push(egui::Event::PointerButton {
                        pos,
                        button,
                        pressed: *state == ElementState::Pressed,
                        modifiers: self.modifiers,
                    });
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (unit, delta) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        (egui::MouseWheelUnit::Line, egui::vec2(*x, *y))
                    }
                    MouseScrollDelta::PixelDelta(p) => (
                        egui::MouseWheelUnit::Point,
                        egui::vec2(p.x as f32 / ppp, p.y as f32 / ppp),
                    ),
                };
                self.events.push(egui::Event::MouseWheel {
                    unit,
                    delta,
                    phase: egui::TouchPhase::Move,
                    modifiers: self.modifiers,
                });
            }
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                self.modifiers = egui::Modifiers {
                    alt: s.alt_key(),
                    ctrl: s.control_key(),
                    shift: s.shift_key(),
                    mac_cmd: false,
                    command: s.control_key(),
                };
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if let Some(key) = egui_key(&event.logical_key) {
                    self.events.push(egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: event.repeat,
                        modifiers: self.modifiers,
                    });
                }
                if pressed && !self.modifiers.ctrl && !self.modifiers.alt {
                    if let Some(text) = &event.text {
                        if text.chars().all(|c| !c.is_control()) {
                            self.events.push(egui::Event::Text(text.to_string()));
                        }
                    }
                }
            }
            WindowEvent::Focused(f) => {
                self.focused = *f;
                self.events.push(egui::Event::WindowFocused(*f));
            }
            _ => {}
        }
    }

    fn take(&mut self, width: u32, height: u32, ppp: f32, time: f64) -> egui::RawInput {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32 / ppp, height as f32 / ppp),
            )),
            max_texture_side: Some(8192),
            time: Some(time),
            predicted_dt: 1.0 / 60.0,
            modifiers: self.modifiers,
            events: std::mem::take(&mut self.events),
            focused: self.focused,
            ..Default::default()
        };
        raw.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(ppp);
        raw
    }
}

fn egui_key(key: &winit::keyboard::Key) -> Option<egui::Key> {
    use winit::keyboard::Key;
    match key {
        Key::Character(s) => egui::Key::from_name(&s.to_uppercase()),
        Key::Named(n) => egui::Key::from_name(&format!("{n:?}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{Key, NamedKey, SmolStr};

    #[test]
    fn keys_map() {
        assert_eq!(
            egui_key(&Key::Named(NamedKey::Backspace)),
            Some(egui::Key::Backspace)
        );
        assert_eq!(
            egui_key(&Key::Named(NamedKey::ArrowLeft)),
            Some(egui::Key::ArrowLeft)
        );
        assert_eq!(
            egui_key(&Key::Named(NamedKey::Enter)),
            Some(egui::Key::Enter)
        );
        assert_eq!(
            egui_key(&Key::Character(SmolStr::new("a"))),
            Some(egui::Key::A)
        );
        assert_eq!(
            egui_key(&Key::Character(SmolStr::new("7"))),
            Some(egui::Key::Num7)
        );
    }

    #[test]
    fn input_scales_by_pixels_per_point() {
        let mut input = Input::default();
        input.on_event(
            &WindowEvent::CursorMoved {
                device_id: winit::event::DeviceId::dummy(),
                position: winit::dpi::PhysicalPosition::new(200.0, 100.0),
            },
            2.0,
        );
        let raw = input.take(800, 600, 2.0, 1.0);
        assert_eq!(raw.screen_rect.unwrap().max, egui::pos2(400.0, 300.0));
        assert!(
            matches!(raw.events[0], egui::Event::PointerMoved(p) if p == egui::pos2(100.0, 50.0))
        );
        assert!(input.events.is_empty());
    }
}
