//! `packages/ui/src/demos/animation`: the animation gallery, ported tile by tile. Each tile
//! has a replay button that remounts its content (Remix's `Tile` with `remountKey`).
//!
//! Not ported: the tiles built on `@keyframes` in `css()` (rotate, keyframes), the ones
//! that are mostly pointer physics (cube, drag-release, material ripple, hold to confirm,
//! shared layout, multi-state badge, aspect ratio, color interpolation, interruptible
//! keyframes) and press.

use rmx::{css, on, rsx, Children, Component, Handle, Mixin, Node};
use rmx::ui::animation::{
    animate_entrance, animate_exit, animate_layout, spring, Animate, Layout, SpringOptions, SpringPreset,
};

/// `spring({ duration, bounce })`.
fn spring_options(duration: f64, bounce: f64) -> SpringOptions {
    SpringOptions { duration, bounce, velocity: 0.0 }
}

#[derive(Clone, Default)]
pub struct TileProps {
    pub title: &'static str,
    pub id: &'static str,
    pub children: Children,
}

/// A gallery card whose replay button remounts the content.
pub struct Tile {
    remount: u32,
}

impl Component for Tile {
    type Props = TileProps;

    fn setup(_: &TileProps, _: &Handle<Self>) -> Self {
        Tile { remount: 0 }
    }

    fn render(&self, props: &TileProps, h: &Handle<Self>) -> Node {
        rsx! {
            <div id={props.id} class="tile" mix={css("background-color: white; padding: 40px; border-radius: 12px; box-shadow: 0 0 10px 0 rgba(0, 0, 0, 0.1); display: flex; align-items: center; flex-direction: column; gap: 12px; position: relative;")}>
                <button class="replay" title="Replay animation" mix={[
                    css("position: absolute; bottom: 8px; right: 8px; width: 18px; height: 18px; padding: 0; border: none; background: transparent; cursor: pointer; opacity: 0.4; &:hover { opacity: 1; }"),
                    on("click", h.on(|this, h| {
                        this.remount += 1;
                        h.update();
                    })),
                ]}>
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M1 4v6h6M23 20v-6h-6"/>
                        <path d="M20.49 9A9 9 0 0 0 5.64 5.64L1 10m22 4l-4.64 4.36A9 9 0 0 1 3.51 15"/>
                    </svg>
                </button>
                <h3 mix={css("margin: 0;")}>{props.title}</h3>
                <div key={self.remount} mix={css("flex: 1; display: flex; align-items: center; justify-content: center; min-height: 200px;")}>
                    {&props.children}
                </div>
            </div>
        }
    }
}

pub struct AnimationGallery;

impl Component for AnimationGallery {
    type Props = ();
    rmx::client_entry!("AnimationGallery");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        AnimationGallery
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <div mix={css("display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 24px; padding: 24px; background-color: #f5f5f5;")}>
                <Tile title="Enter" id="enter"><EnterAnimation/></Tile>
                <Tile title="Exit" id="exit"><ExitAnimation/></Tile>
                <Tile title="Presence list" id="presence"><MixinPresenceList/></Tile>
                <Tile title="Reclaim mid-exit" id="reclaim"><MixinReclaim/></Tile>
                <Tile title="Default animate" id="default-animate"><DefaultAnimate/></Tile>
                <Tile title="Reordering" id="reordering"><Reordering/></Tile>
                <Tile title="FLIP toggle" id="flip"><FlipToggle/></Tile>
                <Tile title="Bouncy switch" id="bouncy"><BouncySwitch/></Tile>
                <Tile title="Rolling square" id="rolling"><RollingSquare/></Tile>
                <Tile title="Transition options" id="transition-options"><TransitionOptions/></Tile>
                <Tile title="HTML content" id="html-content"><HtmlContent/></Tile>
            </div>
        }
    }
}

// enter.tsx
pub struct EnterAnimation;

impl Component for EnterAnimation {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        EnterAnimation
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <div class="ball" mix={[
                css("width: 100px; height: 100px; background-color: #dd00ee; border-radius: 50%;"),
                animate_entrance(Animate::spring(spring_options(400.0, 0.5)).opacity(0.0).transform("scale(0)")),
            ]}/>
        }
    }
}

// exit.tsx
pub struct ExitAnimation {
    visible: bool,
    should_animate: bool,
}

impl Component for ExitAnimation {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        // Skip the entrance on the first render; later shows animate in.
        h.queue_task(|this, _| this.should_animate = true);
        ExitAnimation { visible: true, should_animate: false }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let card = self.visible.then(|| {
            rsx! {
                <div key="exit-animation" class="card" mix={[
                    css("width: 100px; height: 100px; background-color: #0cdcf7; border-radius: 10px;"),
                    animate_entrance(self.should_animate.then(|| Animate::spring(SpringPreset::Snappy).opacity(0.0).transform("scale(0)"))),
                    animate_exit(Animate::spring(SpringPreset::Snappy).opacity(0.0).transform("scale(0)")),
                ]}/>
            }
        });
        rsx! {
            <div mix={css("display: flex; flex-direction: column; width: 100px; height: 160px; position: relative;")}>
                {card}
                <button class="toggle" mix={[
                    css("background-color: #0cdcf7; border-radius: 10px; padding: 10px 20px; color: #0f1115; border: none; cursor: pointer; position: absolute; bottom: 0; left: 0; right: 0; transition: transform 100ms ease-in-out; &:active { transform: translateY(1px); }"),
                    on("click", h.on(|this, h| {
                        this.visible = !this.visible;
                        h.update();
                    })),
                ]}>{if self.visible { "Hide" } else { "Show" }}</button>
            </div>
        }
    }
}

fn small_button(background: &str, hover: &str) -> Mixin {
    css(&format!("flex: 1; padding: 8px 10px; border: none; border-radius: 6px; background-color: {background}; color: white; cursor: pointer; &:hover {{ background-color: {hover}; }}"))
}

fn row_css() -> Mixin {
    css("display: flex; align-items: center; justify-content: space-between; padding: 10px 12px; border-radius: 8px; background-color: #f8fafc; color: #334155; border: 1px solid #e2e8f0;")
}

fn close_button_css() -> Mixin {
    css("width: 24px; height: 24px; padding: 0; border: none; border-radius: 4px; background-color: transparent; color: #64748b; cursor: pointer; &:hover { background-color: #e2e8f0; }")
}

// mixin-presence-list.tsx
pub struct MixinPresenceList {
    items: Vec<u32>,
    next_id: u32,
}

impl Component for MixinPresenceList {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        MixinPresenceList { items: vec![1, 2, 3], next_id: 4 }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let rows: Vec<Node> = self
            .items
            .iter()
            .map(|&id| {
                rsx! {
                    <div key={id} class="row" mix={[
                        animate_entrance(Animate::new(220.0).easing("ease-out").opacity(0.0).transform("translateY(-10px) scale(0.98)")),
                        animate_exit(Animate::new(180.0).easing("ease-in").opacity(0.0).transform("translateY(10px) scale(0.98)")),
                        row_css(),
                    ]}>
                        <span>{format!("Row {id}")}</span>
                        <button class="close" mix={[close_button_css(), on("click", h.on(move |this, h| {
                            this.items.retain(|i| *i != id);
                            h.update();
                        }))]}>"×"</button>
                    </div>
                }
            })
            .collect();
        rsx! {
            <div class="list" mix={css("display: flex; flex-direction: column; gap: 10px; width: 220px;")}>
                <div mix={css("display: flex; gap: 8px;")}>
                    <button class="add" mix={[small_button("#0ea5e9", "#0284c7"), on("click", h.on(|this, h| {
                        this.items.insert(0, this.next_id);
                        this.next_id += 1;
                        h.update();
                    }))]}>"Add"</button>
                    <button class="remove" mix={[small_button("#ef4444", "#dc2626"), on("click", h.on(|this, h| {
                        this.items.pop();
                        h.update();
                    }))]}>"Remove"</button>
                </div>
                {rows}
            </div>
        }
    }
}

// mixin-reclaim.tsx
pub struct MixinReclaim {
    visible: bool,
    /// Bumped to cancel a pending interrupt timer (`clearTimeout`).
    timer: u32,
}

impl Component for MixinReclaim {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        MixinReclaim { visible: true, timer: 0 }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let card = self.visible.then(|| {
            rsx! {
                <div key="reclaim-card" class="card" mix={[
                    animate_entrance(Animate::new(260.0).easing("ease-out").opacity(0.0).transform("translateY(12px) scale(0.94)")),
                    animate_exit(Animate::new(260.0).easing("ease-in").opacity(0.0).transform("translateY(-12px) scale(0.94)")),
                    css("width: 200px; padding: 14px 16px; border-radius: 10px; background: linear-gradient(135deg, #7c3aed, #3b82f6); color: white; text-align: center; font-weight: 600;"),
                ]}>"Reclaim Me Mid-Exit"</div>
            }
        });
        rsx! {
            <div mix={css("display: flex; flex-direction: column; gap: 12px; width: 240px;")}>
                <div mix={css("display: flex; gap: 8px;")}>
                    <button class="show" mix={[small_button("#10b981", "#059669"), on("click", h.on(|this, h| {
                        this.timer += 1;
                        this.visible = true;
                        h.update();
                    }))]}>"Show"</button>
                    <button class="interrupt" mix={[small_button("#f59e0b", "#d97706"), on("click", h.on(|this, h| {
                        this.timer += 1;
                        let timer = this.timer;
                        this.visible = false;
                        h.update();
                        let h2 = h.clone();
                        h.spawn(async move {
                            rmx::browser::sleep(140).await;
                            h2.with(|this, h| {
                                if this.timer == timer {
                                    this.visible = true;
                                    h.update();
                                }
                            });
                        });
                    }))]}>"Interrupt"</button>
                </div>
                <button class="hide" mix={[
                    css("padding: 8px 10px; border: none; border-radius: 6px; background-color: #ef4444; color: white; cursor: pointer; &:hover { background-color: #dc2626; }"),
                    on("click", h.on(|this, h| {
                        this.timer += 1;
                        this.visible = false;
                        h.update();
                    })),
                ]}>"Hide"</button>
                <div class="stage" mix={css("min-height: 100px; display: grid; place-items: center;")}>{card}</div>
            </div>
        }
    }
}

fn shuffle<T>(items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let j = (rmx::browser::random() * (i + 1) as f64) as usize;
        items.swap(i, j);
    }
}

// default-animate.tsx
pub struct DefaultAnimate {
    items: Vec<u32>,
    next_id: u32,
}

impl Component for DefaultAnimate {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        DefaultAnimate { items: vec![1, 2], next_id: 3 }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let rows: Vec<Node> = self
            .items
            .iter()
            .map(|&id| {
                rsx! {
                    <div key={id} class="item" mix={[
                        animate_entrance(true),
                        animate_layout(true),
                        css("display: flex; align-items: center; justify-content: space-between; padding: 12px 16px; background-color: #f1f5f9; border-radius: 8px; font-size: 14px; font-weight: 500; color: #334155;"),
                    ]}>
                        <span>{format!("Item {id}")}</span>
                        <button class="close" mix={[close_button_css(), on("click", h.on(move |this, h| {
                            this.items.retain(|i| *i != id);
                            h.update();
                        }))]}>"×"</button>
                    </div>
                }
            })
            .collect();
        rsx! {
            <div mix={css("display: flex; flex-direction: column; gap: 10px; width: 200px; align-self: flex-start;")}>
                <div mix={css("display: flex; gap: 8px; margin-bottom: 8px;")}>
                    <button class="add" mix={[small_button("#10b981", "#059669"), on("click", h.on(|this, h| {
                        this.items.insert(0, this.next_id);
                        this.next_id += 1;
                        h.update();
                    }))]}>"Add"</button>
                    <button class="shuffle" mix={[small_button("#6366f1", "#4f46e5"), on("click", h.on(|this, h| {
                        shuffle(&mut this.items);
                        h.update();
                    }))]}>"Shuffle"</button>
                </div>
                {rows}
            </div>
        }
    }
}

// reordering.tsx
pub struct Reordering {
    order: Vec<&'static str>,
}

impl Component for Reordering {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.queue_task(|_, h| {
            let h2 = h.clone();
            h.spawn(async move {
                loop {
                    rmx::browser::sleep(1000).await;
                    // Stops once the component is gone (Remix aborts its timer with the signal).
                    let shuffled = h2.with(|this, h| {
                        shuffle(&mut this.order);
                        h.update();
                    });
                    if shuffled.is_none() {
                        break;
                    }
                }
            });
        });
        Reordering { order: vec!["#ff0088", "#dd00ee", "#9911ff", "#0d63f8"] }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let items: Vec<Node> = self
            .order
            .iter()
            .map(|&color| {
                rsx! {
                    <li key={color} style={format!("background-color: {color}")} mix={[
                        css("width: 100px; height: 100px; border-radius: 10px;"),
                        animate_layout(Layout::spring(spring_options(600.0, 0.2))),
                    ]}/>
                }
            })
            .collect();
        rsx! {
            <ul mix={css("list-style: none; padding: 0; margin: 0; position: relative; display: flex; flex-wrap: wrap; gap: 10px; width: 220px; flex-direction: row; justify-content: center; align-items: center;")}>
                {items}
            </ul>
        }
    }
}

// flip-toggle.tsx
pub struct FlipToggle {
    on: bool,
}

impl Component for FlipToggle {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        FlipToggle { on: false }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        rsx! {
            <button class="switch" style={format!("justify-content: {}", if self.on { "flex-start" } else { "flex-end" })} mix={[
                css("width: 90px; height: 50px; background-color: rgba(153, 17, 255, 0.2); border-radius: 50px; cursor: pointer; display: flex; padding: 10px; border: none;"),
                on("click", h.on(|this, h| {
                    this.on = !this.on;
                    h.update();
                })),
            ]}>
                <div class="knob" mix={[
                    css("width: 30px; height: 30px; background-color: #9911ff; border-radius: 50%;"),
                    animate_layout(Layout::new().duration(200.0).easing("ease-in-out")),
                ]}/>
            </button>
        }
    }
}

const BOUNCE_EASING: &str = "linear(0, 0.258 12%, 0.424 18.3%, 0.633 24.4%, 0.999 33.3%, 0.783 39.8%, 0.733 42.5%, 0.716 45.1%, 0.731 47.6%, 0.777 50.2%, 0.999 57.7%, 0.906 61.7%, 0.883 63.5%, 0.876 65.2%, 0.901 68.7%, 0.999 74.5%, 0.964 77.4%, 0.953 80.1%, 0.961 82.6%, 1 88.2%, 0.99 91.9%, 1)";

// bouncy-switch.tsx
pub struct BouncySwitch {
    on: bool,
}

impl Component for BouncySwitch {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        BouncySwitch { on: true }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let style = if self.on {
            format!("transform: translateY(-100px); transition: transform {}", spring(SpringPreset::Snappy))
        } else {
            format!("transform: translateY(0); transition: transform 800ms {BOUNCE_EASING}")
        };
        rsx! {
            <div class="bouncy" mix={[
                css("height: 160px; background-color: #ff6b35; display: flex; flex-direction: column; justify-content: flex-end; border-radius: 50px; padding: 10px; cursor: pointer;"),
                on("click", h.on(|this, h| {
                    this.on = !this.on;
                    h.update();
                })),
            ]}>
                <div class="ball" style={style} mix={css("width: 60px; height: 60px; background-color: white; border-radius: 30px; will-change: transform;")}/>
            </div>
        }
    }
}

// rolling-square.tsx
pub struct RollingSquare {
    toggled: bool,
}

impl Component for RollingSquare {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        RollingSquare { toggled: false }
    }
    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let transform = if self.toggled { "translateX(100%) rotate(180deg)" } else { "translateX(-100%)" };
        rsx! {
            <div mix={css("display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 20px; min-width: 300px;")}>
                <div class="square" style={format!("transform: {transform}")} mix={css(&format!(
                    "width: 80px; height: 80px; background-color: #8df0cc; border-radius: 10px; transition: transform {};",
                    spring(spring_options(500.0, 0.5))
                ))}/>
                <button class="toggle" mix={[
                    css("background-color: #8df0cc; color: #0f1115; border-radius: 5px; padding: 10px; margin: 10px; border: none; cursor: pointer;"),
                    on("click", h.on(|this, h| {
                        this.toggled = !this.toggled;
                        h.update();
                    })),
                ]}>"Toggle position"</button>
            </div>
        }
    }
}

// transition-options.tsx
pub struct TransitionOptions;

impl Component for TransitionOptions {
    type Props = ();
    fn setup(_: &(), _: &Handle<Self>) -> Self {
        TransitionOptions
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! {
            <div class="ball" mix={[
                css("width: 100px; height: 100px; border-radius: 50%; background-color: #9911ff;"),
                animate_entrance(Animate::new(800.0).delay(500.0).easing("cubic-bezier(0, 0.71, 0.2, 1.01)").opacity(0.0).transform("scale(0.5)")),
            ]}/>
        }
    }
}

// html-content.tsx: a spring iterated once per animation frame.
pub struct HtmlContent {
    count: i64,
}

impl Component for HtmlContent {
    type Props = ();
    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.queue_task(|_, h| {
            let h2 = h.clone();
            h.spawn(async move {
                let (from, to) = (0.0, 100.0);
                for t in spring(spring_options(3000.0, 0.0)) {
                    rmx::browser::next_frame().await;
                    let count = (from + (to - from) * t + 0.5).floor() as i64;
                    if h2.with(|this, h| {
                        this.count = count;
                        h.update();
                    })
                    .is_none()
                    {
                        break;
                    }
                }
            });
        });
        HtmlContent { count: 0 }
    }
    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <pre class="count" mix={css("font-size: 64px; margin: 0; color: #8df0cc;")}>{self.count}</pre> }
    }
}
