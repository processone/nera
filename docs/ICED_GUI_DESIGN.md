# Iced GUI Integration — Design Document

## 1. Executive Summary

This document describes a design for integrating the [Iced](https://iced.rs)
GPU-accelerated GUI framework into nera, allowing Erlang applications compiled
with nera to build native desktop interfaces.

The core insight is that Iced's Elm architecture (`init → update → view`) is
structurally identical to Erlang's `gen_server` pattern
(`init → handle_cast → state`). A new `nera_gui` behaviour exposes this loop
to Erlang programmers with zero new concepts: the GUI is just another
process whose state drives a declarative view function.

The Iced event loop runs on the main OS thread (required by macOS/AppKit).
The Erlang scheduler runs on its existing worker threads. A lock-free channel
bridges the two sides. From the Erlang programmer's perspective, the entire
interaction is message passing — exactly like talking to any other process.

---

## 2. Goals and Non-Goals

### Goals

- **Erlang-native API**: GUI applications are written as normal Erlang modules
  implementing a behaviour. No Rust, no foreign syntax.
- **Elm/gen_server loop**: `init/1`, `update/2`, `view/1` callbacks map
  directly to the programmer's existing mental model.
- **Single-binary deployment**: GUI applications compile to one standalone
  executable, just like non-GUI nera programs.
- **Process integration**: the GUI process is a regular Erlang process — it
  has a PID, can be linked/monitored, supervised, and sent messages from
  other processes.
- **GPU-accelerated rendering**: leverage Iced's wgpu backend for smooth
  60 fps rendering without blocking the Erlang scheduler.
- **Cross-platform**: macOS (Metal), Linux (Vulkan/OpenGL), Windows (DX12).
- **Feature-gated**: `cargo` feature flag `gui` — programs that don't use
  GUI pay zero compile-time or binary-size cost.

### Non-Goals

- Full widget toolkit parity with wxWidgets or Qt.
- Web rendering (Iced has experimental web support; we do not target it).
- Hot code reloading of GUI modules at runtime.
- Multiple top-level windows in the first iteration (single window first).
- Custom GPU shaders exposed to Erlang.

---

## 3. Architecture Overview

### 3.1 Threading Model

```
┌─────────────────────────────────────────────────────────────────┐
│                     nera GUI Application                        │
│                                                                 │
│  Main Thread (OS)                                               │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              Iced Event Loop                              │  │
│  │  • Window management (create, resize, close)              │  │
│  │  • Input capture (mouse, keyboard, touch)                 │  │
│  │  • Layout computation                                     │  │
│  │  • GPU rendering (wgpu → Metal / Vulkan / DX12)           │  │
│  │  • Receives view trees from bridge, diffs & renders       │  │
│  └──────────────────────┬────────────────────────────────────┘  │
│                         │ mpsc channels                         │
│  ┌──────────────────────▼────────────────────────────────────┐  │
│  │              GUI Bridge                                   │  │
│  │  • Erlang term ←→ Iced Element conversion                 │  │
│  │  • Event routing: Iced Message → Erlang term              │  │
│  │  • View routing: Erlang term tree → Iced widget tree      │  │
│  └──────────────────────┬────────────────────────────────────┘  │
│                         │ lock-free queue                       │
│  Worker Threads (N)                                             │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              nera Scheduler                               │  │
│  │  • Work-stealing across N cores                           │  │
│  │  • GUI process runs alongside all other Erlang processes  │  │
│  │  • Reduction-based preemption (4000 reductions/slice)     │  │
│  │  • Per-process heaps, copying GC, message passing         │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                 │
│  Timer Thread        │  I/O Pool         │  Reactor (mio)       │
│  (receive…after)     │  (file ops)       │  (TCP)               │
└─────────────────────────────────────────────────────────────────┘
```

Key point: `nera_runtime_start_mfa()` in `entry.rs` currently creates the
scheduler and calls `scheduler.run()`, which spawns worker threads and
**blocks the calling thread** by joining them. For GUI mode, we introduce
`nera_runtime_start_gui_mfa()` which:

1. Spawns the scheduler on a **background thread** (detached join handle).
2. Runs `iced::application(...)::run()` on the **main thread** (required by
   macOS AppKit — `NSApplication` must run on thread 0).
3. On window close, sets the scheduler's `shutdown` flag and joins workers.

This requires no changes to the scheduler itself — only a new entry point.

### 3.2 The GUI Process

The GUI process is a regular Erlang process managed by the scheduler. It has
a PID, a heap, a mailbox, and can be linked/monitored. It differs from
ordinary processes in one way: the bridge holds a reference to it and
periodically calls its `view/1` and `update/2` callbacks.

```
┌──────────────────────────────────────────────────────────────┐
│                     GUI Process (Erlang)                      │
│                                                              │
│  State = Module:init(Args)     ← called once at startup     │
│                                                              │
│  loop:                                                       │
│    receive                                                   │
│      {gui_event, Message} →                                  │
│        State1 = Module:update(Message, State)                │
│        ViewTree = Module:view(State1)                        │
│        send bridge ← ViewTree                                │
│        loop(State1)                                          │
│                                                              │
│      {gui_command, From, Cmd} →                              │
│        handle application-level commands                     │
│        loop(State)                                           │
│                                                              │
│      AnyOtherMsg →                                           │
│        State1 = Module:handle_info(AnyOtherMsg, State)       │
│        ViewTree = Module:view(State1)                        │
│        send bridge ← ViewTree                                │
│        loop(State1)                                          │
│    end                                                       │
└──────────────────────────────────────────────────────────────┘
```

### 3.3 Update Cycle (Detailed)

```
 User clicks button          Iced captures event
 ──────────────────────────────────────────────────────────────►
                              │
                              ▼
                    Bridge extracts the Erlang
                    message term associated with
                    the button (e.g. atom `increment`)
                              │
                              ▼
                    Sends {gui_event, increment}
                    to the GUI process mailbox
                              │
                              ▼
              GUI process wakes (scheduler picks it up)
              Calls Module:update(increment, State)
              Gets NewState
              Calls Module:view(NewState)
              Gets ViewTree (nested Erlang tuples)
                              │
                              ▼
                    Bridge receives ViewTree
                    Walks term tree, builds Iced Elements
                    (diff against previous tree optional — Phase 2)
                              │
                              ▼
                    Iced re-renders the window
 ◄──────────────────────────────────────────────────────────────
 Updated pixels on screen
```

---

## 4. Erlang API

### 4.1 The `nera_gui` Behaviour

```erlang
-module(nera_gui).

%% Behaviour callbacks
-callback init(Args :: term()) ->
    {ok, State :: term()} |
    {ok, State :: term(), Opts :: [gui_opt()]}.

-callback update(Message :: term(), State :: term()) ->
    {ok, NewState :: term()} |
    {ok, NewState :: term(), [command()]}.

-callback view(State :: term()) ->
    gui:element().

%% Optional callbacks
-callback handle_info(Msg :: term(), State :: term()) ->
    {ok, NewState :: term()} |
    {ok, NewState :: term(), [command()]}.

-callback subscriptions(State :: term()) ->
    [subscription()].

%% Types
-type gui_opt()      :: {title, string()}
                      | {size, {integer(), integer()}}
                      | {resizable, boolean()}
                      | {decorations, boolean()}
                      | {theme, light | dark}.
-type command()      :: {spawn, fun()} | {send, pid(), term()}.
-type subscription() :: {tick, Millis :: integer(), Message :: term()}.
```

### 4.2 Entry Point

```erlang
%% Start a GUI application (called from generated main)
nera_gui:run(Module, Args) ->
    nera_gui:run(Module, Args, []).

nera_gui:run(Module, Args, Opts) ->
    %% This is a special FFI call that:
    %% 1. Starts the scheduler on a background thread
    %% 2. Spawns the GUI process
    %% 3. Runs Iced on the main thread
    %% 4. Returns when the window closes
    nera_gui_ffi:run(Module, Args, Opts).
```

### 4.3 The `gui` Widget Module

All widget functions are **pure constructors** — they return Erlang terms
(nested tuples) that describe the UI. No side effects, no FFI calls. The
runtime interprets these terms on the Rust side.

```erlang
-module(gui).

%% Layout
-export([column/1, column/2, row/1, row/2, stack/1]).
%% Display
-export([text/1, text/2, image/1, image/2]).
%% Input
-export([button/2, button/3,
         text_input/2, text_input/3,
         checkbox/3, checkbox/4,
         slider/4,
         toggler/3,
         pick_list/3, pick_list/4]).
%% Containers
-export([scrollable/1, container/1, container/2]).
%% Decorators
-export([padding/2, spacing/2, width/2, height/2, align/2]).

%% --- Layout ---

column(Children) when is_list(Children) ->
    {column, Children, []}.

column(Children, Opts) when is_list(Children), is_list(Opts) ->
    {column, Children, Opts}.

row(Children) when is_list(Children) ->
    {row, Children, []}.

row(Children, Opts) when is_list(Children), is_list(Opts) ->
    {row, Children, Opts}.

%% --- Display ---

text(Content) when is_list(Content) ->
    {text, Content, []}.

text(Content, Opts) when is_list(Content), is_list(Opts) ->
    {text, Content, Opts}.

%% --- Input ---

%% button(Label, OnPressMessage)
%% When clicked, OnPressMessage is sent to update/2
button(Label, OnPress) ->
    {button, Label, OnPress, []}.

button(Label, OnPress, Opts) ->
    {button, Label, OnPress, Opts}.

%% text_input(Placeholder, Value)
%% Emits {text_changed, NewValue} by default,
%% or the message from {on_change, fun(V) -> Msg end}
text_input(Placeholder, Value) ->
    {text_input, Placeholder, Value, []}.

text_input(Placeholder, Value, Opts) ->
    {text_input, Placeholder, Value, Opts}.

%% checkbox(Label, IsChecked, OnToggleMsg)
checkbox(Label, IsChecked, OnToggle) ->
    {checkbox, Label, IsChecked, OnToggle, []}.

%% slider(Min, Max, Value, OnChangeMsg)
slider(Min, Max, Value, OnChange) ->
    {slider, Min, Max, Value, OnChange, []}.

%% pick_list(Options, Selected, OnSelectMsg)
%% Options = [string()], Selected = string() | undefined
pick_list(Options, Selected, OnSelect) ->
    {pick_list, Options, Selected, OnSelect, []}.

%% --- Decorators (widget transformers) ---

padding(Widget, Px) ->
    {padding, Widget, Px}.

spacing(Widget, Px) ->
    {spacing, Widget, Px}.

width(Widget, W) ->
    {width, Widget, W}.

height(Widget, H) ->
    {height, Widget, H}.
```

### 4.4 Complete Example: Counter

```erlang
-module(counter).
-behaviour(nera_gui).

-export([main/0]).
-export([init/1, update/2, view/1]).

main() ->
    nera_gui:run(?MODULE, [], [{title, "Counter"}]).

init(_Args) ->
    {ok, #{count => 0}}.

update(increment, #{count := N} = State) ->
    {ok, State#{count := N + 1}};
update(decrement, #{count := N} = State) ->
    {ok, State#{count := N - 1}};
update(reset, State) ->
    {ok, State#{count := 0}}.

view(#{count := Count}) ->
    gui:padding(
        gui:column([
            gui:text("Counter: " ++ integer_to_list(Count), [{size, 32}]),
            gui:row([
                gui:button("- Decrement", decrement),
                gui:button("Reset", reset),
                gui:button("+ Increment", increment)
            ], [{spacing, 10}])
        ], [{spacing, 20}, {align, center}]),
    40).
```

### 4.5 Complete Example: Todo List

```erlang
-module(todo).
-behaviour(nera_gui).

-export([main/0]).
-export([init/1, update/2, view/1]).

main() ->
    nera_gui:run(?MODULE, [], [{title, "Todo"}, {size, {400, 600}}]).

init(_Args) ->
    {ok, #{
        items => [],
        input => "",
        next_id => 1
    }}.

update({input_changed, Text}, State) ->
    {ok, State#{input := Text}};

update(add_item, #{input := "", items := _} = State) ->
    {ok, State};  %% ignore empty input

update(add_item, #{input := Text, items := Items, next_id := Id} = State) ->
    Item = #{id => Id, text => Text, done => false},
    {ok, State#{
        items := Items ++ [Item],
        input := "",
        next_id := Id + 1
    }};

update({toggle, Id}, #{items := Items} = State) ->
    Items1 = lists:map(
        fun(#{id := ItemId, done := Done} = Item) when ItemId =:= Id ->
                Item#{done := not Done};
           (Item) ->
                Item
        end, Items),
    {ok, State#{items := Items1}};

update({delete, Id}, #{items := Items} = State) ->
    Items1 = lists:filter(
        fun(#{id := ItemId}) -> ItemId =/= Id end, Items),
    {ok, State#{items := Items1}}.

view(#{items := Items, input := Input}) ->
    gui:padding(
        gui:column([
            gui:text("Todo List", [{size, 24}]),
            gui:row([
                gui:text_input("What needs to be done?", Input,
                    [{on_change, fun(V) -> {input_changed, V} end},
                     {on_submit, add_item}]),
                gui:button("Add", add_item)
            ], [{spacing, 10}]),
            gui:scrollable(
                gui:column(
                    [view_item(Item) || Item <- Items],
                    [{spacing, 5}]
                )
            )
        ], [{spacing, 15}]),
    20).

view_item(#{id := Id, text := Text, done := Done}) ->
    gui:row([
        gui:checkbox("", Done, {toggle, Id}),
        gui:text(Text, [{strikethrough, Done}]),
        gui:button("X", {delete, Id}, [{style, danger}])
    ], [{spacing, 10}]).
```

### 4.6 Example: Multi-Process GUI

This example demonstrates the key advantage — the GUI process communicates
with worker processes via standard Erlang message passing.

```erlang
-module(downloader).
-behaviour(nera_gui).

-export([main/0]).
-export([init/1, update/2, view/1, handle_info/2]).

main() ->
    nera_gui:run(?MODULE, [], [{title, "Downloader"}]).

init(_Args) ->
    {ok, #{status => idle, progress => 0}}.

update(start_download, State) ->
    %% Spawn a worker process that sends progress updates back
    Self = self(),
    spawn_link(fun() -> download_worker(Self) end),
    {ok, State#{status := downloading, progress := 0}};

update(_, State) ->
    {ok, State}.

handle_info({progress, Pct}, State) ->
    {ok, State#{progress := Pct}};
handle_info(download_complete, State) ->
    {ok, State#{status := done, progress := 100}}.

view(#{status := idle}) ->
    gui:column([
        gui:text("Ready to download"),
        gui:button("Start", start_download)
    ]);

view(#{status := downloading, progress := Pct}) ->
    gui:column([
        gui:text("Downloading... " ++ integer_to_list(Pct) ++ "%"),
        gui:progress_bar(Pct, 100)
    ]);

view(#{status := done}) ->
    gui:column([
        gui:text("Download complete!"),
        gui:button("Download Again", start_download)
    ]).

%% --- Worker (runs in a separate Erlang process) ---

download_worker(GuiPid) ->
    lists:foreach(
        fun(I) ->
            timer:sleep(50),
            GuiPid ! {progress, I * 10}
        end,
        lists:seq(1, 10)),
    GuiPid ! download_complete.
```

---

## 5. Rust Implementation

### 5.1 New Files

| File | Crate | Purpose |
|------|-------|---------|
| `gui_bridge.rs` | `nera-runtime` | Channel types, term↔widget conversion, view diffing |
| `gui_app.rs` | `nera-runtime` | `impl iced::Application` delegating to Erlang callbacks |
| `ffi/gui.rs` | `nera-runtime` | FFI entry points: `nera_rt_gui_run`, widget constructors |
| `gui.erl` | `erlang/stdlib` | Pure Erlang widget API (Section 4.3) |
| `nera_gui.erl` | `erlang/stdlib` | Behaviour definition + `run/3` wrapper |

### 5.2 Cargo Feature Flag

```toml
# crates/nera-runtime/Cargo.toml
[features]
default = []
gui = ["iced"]

[dependencies]
iced = { version = "0.13", optional = true, default-features = false, features = ["wgpu"] }
```

Programs that don't use GUI features compile without iced — no binary bloat,
no GPU dependency.

### 5.3 GUI Bridge

The bridge is the central piece. It owns two channels:

```rust
// gui_bridge.rs

use std::sync::mpsc;

/// Messages from Iced → Erlang GUI process
pub enum ToErlang {
    /// A user interaction produced this Erlang term (e.g. atom `increment`)
    GuiEvent(u64),  // encoded ErlTerm
    /// Window was closed
    WindowClosed,
}

/// Messages from Erlang GUI process → Iced
pub enum ToIced {
    /// New view tree (Erlang term to be converted to widgets)
    ViewTree(ViewNode),
    /// Update window title
    SetTitle(String),
}

/// Parsed view tree — intermediate representation between Erlang terms
/// and Iced widgets. Avoids holding raw Erlang terms (which reference
/// a process heap that could be GC'd).
pub enum ViewNode {
    Text { content: String, opts: TextOpts },
    Button { label: String, on_press: u64, opts: ButtonOpts },
    Column { children: Vec<ViewNode>, opts: LayoutOpts },
    Row { children: Vec<ViewNode>, opts: LayoutOpts },
    TextInput { placeholder: String, value: String, on_change: u64, opts: InputOpts },
    Checkbox { label: String, checked: bool, on_toggle: u64 },
    Slider { min: f64, max: f64, value: f64, on_change: u64 },
    Scrollable { child: Box<ViewNode> },
    Container { child: Box<ViewNode>, opts: ContainerOpts },
    ProgressBar { value: f64, max: f64 },
    Padding { child: Box<ViewNode>, amount: u16 },
    // ... more widgets as needed
}
```

### 5.4 Term Tree → ViewNode Conversion

The Erlang `view/1` function returns a nested tuple tree like:

```erlang
{column, [{text, "Hello", []}, {button, "Click", my_msg, []}], []}
```

The Rust bridge walks this term tree and produces a `ViewNode`:

```rust
// gui_bridge.rs

fn term_to_view_node(heap: &ProcessHeap, term: u64) -> Result<ViewNode, GuiError> {
    let t = ErlTerm(term);
    if !t.is_tuple() {
        return Err(GuiError::InvalidViewTree);
    }
    let elements = t.tuple_elements(heap);
    let tag = elements[0].as_atom_id().ok_or(GuiError::InvalidViewTree)?;

    match atom_table.resolve(tag).as_deref() {
        Some("text") => {
            let content = term_to_string(heap, elements[1])?;
            let opts = parse_text_opts(heap, elements[2])?;
            Ok(ViewNode::Text { content, opts })
        }
        Some("button") => {
            let label = term_to_string(heap, elements[1])?;
            let on_press = elements[2].0;  // raw term, sent back on click
            let opts = parse_button_opts(heap, elements[3])?;
            Ok(ViewNode::Button { label, on_press, opts })
        }
        Some("column") => {
            let children = term_list_to_vec(heap, elements[1])?
                .into_iter()
                .map(|child| term_to_view_node(heap, child))
                .collect::<Result<Vec<_>, _>>()?;
            let opts = parse_layout_opts(heap, elements[2])?;
            Ok(ViewNode::Column { children, opts })
        }
        // ... other widgets
        _ => Err(GuiError::UnknownWidget(tag))
    }
}
```

### 5.5 ViewNode → Iced Element Conversion

```rust
// gui_app.rs

fn view_node_to_element<'a>(node: &ViewNode) -> Element<'a, GuiMessage> {
    match node {
        ViewNode::Text { content, opts } => {
            let mut t = iced::widget::text(content.clone());
            if let Some(size) = opts.size { t = t.size(size); }
            t.into()
        }
        ViewNode::Button { label, on_press, opts } => {
            let msg = GuiMessage::ErlangTerm(*on_press);
            iced::widget::button(iced::widget::text(label.clone()))
                .on_press(msg)
                .into()
        }
        ViewNode::Column { children, opts } => {
            let mut col = iced::widget::column(
                children.iter().map(view_node_to_element).collect()
            );
            if let Some(s) = opts.spacing { col = col.spacing(s); }
            if let Some(a) = opts.align { col = col.align_x(a); }
            col.into()
        }
        // ... other widgets
    }
}
```

### 5.6 Iced Application Implementation

```rust
// gui_app.rs

struct NeraGuiApp {
    /// Current view tree (rebuilt on each update cycle)
    view_tree: ViewNode,
    /// Channel to send events to the Erlang GUI process
    to_erlang: mpsc::Sender<ToErlang>,
    /// Channel to receive view trees from the Erlang GUI process
    from_erlang: mpsc::Receiver<ToIced>,
    /// Window title
    title: String,
}

#[derive(Debug, Clone)]
enum GuiMessage {
    /// An Erlang term to send to the GUI process's update/2
    ErlangTerm(u64),
    /// A text input changed — carries the closure reference and new value
    TextChanged(u64, String),
    /// Poll for new view trees from Erlang side
    Tick,
}

impl iced::Application for NeraGuiApp {
    type Message = GuiMessage;
    type Theme = iced::Theme;
    type Executor = iced::executor::Default;
    type Flags = GuiFlags;

    fn new(flags: GuiFlags) -> (Self, iced::Command<GuiMessage>) {
        let app = NeraGuiApp {
            view_tree: flags.initial_view,
            to_erlang: flags.to_erlang,
            from_erlang: flags.from_erlang,
            title: flags.title,
        };
        (app, iced::Command::none())
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn update(&mut self, message: GuiMessage) -> iced::Command<GuiMessage> {
        match message {
            GuiMessage::ErlangTerm(term) => {
                // Forward the event to the Erlang GUI process
                let _ = self.to_erlang.send(ToErlang::GuiEvent(term));
            }
            GuiMessage::TextChanged(callback, value) => {
                // Apply the Erlang closure to get the message, then forward
                // (handled via special FFI call)
            }
            GuiMessage::Tick => {
                // Check for new view tree from Erlang
                while let Ok(msg) = self.from_erlang.try_recv() {
                    match msg {
                        ToIced::ViewTree(tree) => self.view_tree = tree,
                        ToIced::SetTitle(t) => self.title = t,
                    }
                }
            }
        }
        iced::Command::none()
    }

    fn view(&self) -> Element<GuiMessage> {
        view_node_to_element(&self.view_tree)
    }

    fn subscription(&self) -> iced::Subscription<GuiMessage> {
        // Poll for Erlang view updates at 60fps
        iced::time::every(Duration::from_millis(16))
            .map(|_| GuiMessage::Tick)
    }
}
```

### 5.7 Entry Point

```rust
// entry.rs (addition)

#[cfg(feature = "gui")]
#[no_mangle]
pub extern "C" fn nera_runtime_start_gui_mfa(
    mod_atom: u64,
    fun_atom: u64,
    gui_mod_atom: u64,
    gui_opts: u64,
) {
    use crate::gui_bridge::{ToErlang, ToIced};
    use crate::gui_app::{NeraGuiApp, GuiFlags};

    let _ = global_atom_table();
    let _ = global_function_table();

    // Create bridge channels
    let (to_erlang_tx, to_erlang_rx) = std::sync::mpsc::channel();
    let (to_iced_tx, to_iced_rx) = std::sync::mpsc::channel();

    // Start the scheduler on a background thread
    let scheduler_handle = std::thread::Builder::new()
        .name("nera-scheduler-main".into())
        .spawn(move || {
            let num_threads = num_cpus::get().max(1);
            let scheduler = Scheduler::new(num_threads);

            // Spawn the GUI process (runs Module:init, enters receive loop)
            let pid = scheduler.next_pid();
            let gui_process = Process::with_gui_bridge(
                pid, gui_mod_atom, to_erlang_rx, to_iced_tx
            );
            scheduler.spawn_process(gui_process);

            // Also spawn the user's entry function if provided
            // (e.g., for background workers)

            scheduler.run()
        })
        .expect("failed to spawn scheduler thread");

    // Parse window options from Erlang term
    let title = parse_title_opt(gui_opts).unwrap_or("nera".to_string());
    let size = parse_size_opt(gui_opts).unwrap_or((800, 600));

    // Run Iced on the main thread (blocks until window closes)
    let flags = GuiFlags {
        initial_view: ViewNode::Text {
            content: "Loading...".into(),
            opts: Default::default()
        },
        to_erlang: to_erlang_tx,
        from_erlang: to_iced_rx,
        title,
    };

    iced::application("nera", NeraGuiApp::update, NeraGuiApp::view)
        .window_size(iced::Size::new(size.0 as f32, size.1 as f32))
        .run_with(|| (NeraGuiApp::new(flags), iced::Task::none()))
        .expect("failed to run iced application");

    // Window closed — signal scheduler shutdown
    // (scheduler thread will join automatically)
}
```

---

## 6. Term Encoding for Messages

When the user clicks a button associated with the atom `increment`, the Iced
side needs to send this atom back to the Erlang GUI process. The challenge:
Erlang terms reference process-local heaps.

**Solution**: For simple message terms (atoms, small integers, tuples of
atoms/integers), we deep-copy them into a standalone buffer at view-build
time. This is the same mechanism nera already uses for inter-process message
passing (`message.rs`).

For closures (e.g., `{on_change, fun(V) -> {input_changed, V} end}`), the
bridge stores the closure term and applies it on the Erlang side when the
event fires — the Iced thread never evaluates Erlang closures.

```
Button click → Iced sends raw atom u64 → Bridge delivers to GUI process inbox
Text input  → Iced sends {closure_ref, new_string} → Bridge applies closure
              on Erlang worker thread → delivers result to GUI process inbox
```

---

## 7. Compilation and Linking

### 7.1 Feature Detection

The codegen crate detects GUI usage at compile time:

```
1. Parser sees: -behaviour(nera_gui).
2. IR lowering flags the module as GUI-enabled.
3. Codegen emits nera_runtime_start_gui_mfa() call instead of
   nera_runtime_start_mfa() in the generated main().
4. Linker links against nera-runtime compiled with the `gui` feature.
```

### 7.2 Build Command

```bash
# Normal (no GUI)
nera build my_app.erl

# GUI application (automatic if module uses nera_gui behaviour)
nera build counter.erl

# Explicit feature flag override
nera build --features gui counter.erl
```

### 7.3 Binary Size Impact

| Build | Approximate Size |
|-------|-----------------|
| Minimal (hello world) | ~4 MB |
| With GUI (iced + wgpu) | ~15-25 MB |
| BEAM + wx (for reference) | ~56 MB + wx libs |

The GUI build is larger due to wgpu shaders and Iced widget code, but still
significantly smaller than BEAM + wx.

---

## 8. Phased Implementation Roadmap

### Phase 1: Proof of Concept (Counter)

**Goal**: Get the counter example running end-to-end.

- [ ] Add `iced` dependency behind `gui` cargo feature
- [ ] Implement `gui_bridge.rs` with `ViewNode` enum and term→node conversion
      for `text`, `button`, `column`, `row`
- [ ] Implement `gui_app.rs` with basic `iced::Application`
- [ ] Add `nera_runtime_start_gui_mfa()` entry point in `entry.rs`
- [ ] Write `gui.erl` with `text/1,2`, `button/2,3`, `column/1,2`, `row/1,2`
- [ ] Write `nera_gui.erl` behaviour definition and process loop
- [ ] Add `ffi/gui.rs` with FFI functions for bridge communication
- [ ] Modify codegen to detect `nera_gui` behaviour and emit GUI entry point
- [ ] Ship the counter example

**Widgets**: `text`, `button`, `column`, `row`, `padding`

### Phase 2: Input Widgets

**Goal**: Text input, checkboxes, sliders — enough for forms.

- [ ] `text_input` with `on_change` closure support
- [ ] `checkbox`, `toggler`, `slider`
- [ ] `pick_list` (dropdown)
- [ ] `scrollable` container
- [ ] `container` with alignment/width/height
- [ ] View tree diffing (avoid full rebuild on every update)
- [ ] Ship the todo example

**Widgets**: `text_input`, `checkbox`, `toggler`, `slider`, `pick_list`,
`scrollable`, `container`

### Phase 3: Styling and Theming

**Goal**: Customizable appearance.

- [ ] Style properties on all widgets (colors, borders, fonts)
- [ ] Light/dark theme support via `gui_opt()`
- [ ] Custom theme definition from Erlang (color palette as a map)
- [ ] `progress_bar`, `tooltip`, `rule` (horizontal/vertical line)

### Phase 4: Advanced Features

**Goal**: Production-ready GUI toolkit.

- [ ] Multiple windows (each window = separate GUI process)
- [ ] `canvas` widget for custom drawing (Erlang drawing commands)
- [ ] File dialogs (`command()` type)
- [ ] Clipboard access
- [ ] Keyboard shortcuts / hotkeys
- [ ] `subscriptions` callback (timers, window events)
- [ ] Supervisor integration (restart GUI on crash)
- [ ] Menu bar support (macOS native menus)

### Phase 5: Ecosystem

- [ ] Example gallery (calculator, file browser, chat client)
- [ ] Documentation with widget catalog
- [ ] Performance benchmarks vs. wx

---

## 9. Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Iced API churn (pre-1.0) | Breaking changes on upgrade | Pin to specific iced version, wrap all usage behind `ViewNode` abstraction |
| Main thread constraint on macOS | Architectural | Already handled: Iced gets main thread, scheduler runs on workers |
| View rebuild cost for large UIs | Latency | Phase 2 introduces view diffing; only changed subtrees rebuild |
| Closure lifecycle (GC vs. Iced references) | Use-after-free | Deep-copy message terms at view-build time; closures applied on Erlang side only |
| wgpu GPU driver compatibility | Runtime crash on old hardware | Iced supports software fallback; detect and warn at startup |
| Binary size increase | Deployment concern | Feature-gated — only linked when used |

---

## 10. Alternatives Considered

| Alternative | Why Not |
|-------------|---------|
| **GPUI** (Zed) | Internal to Zed, unstable API, Rust ownership model doesn't cross FFI cleanly |
| **egui** | Immediate mode — requires calling back into Erlang 60x/sec for the full UI, poor fit for message-passing model |
| **Slint** | Good FFI story but DSL-based — would require a second language alongside Erlang |
| **Tauri/WebView** | Viable (and simpler) but adds web runtime dependency, not truly native rendering |
| **wx (status quo)** | Works on BEAM but requires wxWidgets C++ library; nera aims for single-binary deployment |

Iced was chosen because its architecture is the closest match to Erlang's
actor model, it's pure Rust (no C++ dependencies), GPU-accelerated, and
cross-platform.

---

## 11. Open Questions

1. **Hot view reload**: Could we watch the source file and re-compile/reload
   the `view/1` function without restarting the app? (Requires partial
   recompilation support in nera.)

2. **Accessibility**: Iced's accessibility support is evolving. How much
   do we expose to Erlang?

3. **Native dialogs**: Should file/color/font dialogs be Iced-rendered or
   use OS-native dialogs via `rfd` crate?

4. **Testing**: How to test GUI modules? A headless mode where `view/1`
   returns the term tree without rendering could enable snapshot testing
   of UI state.

5. **Performance target**: What's the acceptable latency budget for the
   update cycle (event → re-render)? Target: < 16ms (60fps) for simple
   apps, < 32ms for complex ones.
