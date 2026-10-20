//! The `MessageStream` client entry for the sse demo (`actions/public/message-stream.tsx`).
//!
//! It opens an `EventSource` after hydration, appends each message to plain component
//! state, and closes the connection when the component is removed.

use rmx::{css, rsx, Component, Handle, IntoNode, Node};
use serde::{Deserialize, Serialize};

/// One server-sent event payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub count: u32,
    pub message: String,
}

/// The messages endpoint, with the limit as a search param.
pub fn messages_href(limit: Option<u32>) -> String {
    match limit {
        Some(limit) => format!("/messages?limit={limit}"),
        None => "/messages".to_string(),
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct MessageStreamProps {
    pub limit: Option<u32>,
}

pub struct MessageStream {
    messages: Vec<Message>,
    connected: bool,
    #[cfg(target_arch = "wasm32")]
    _connection: Option<browser::Connection>,
}

impl Component for MessageStream {
    type Props = MessageStreamProps;
    rmx::client_entry!("MessageStream");

    fn setup(props: &MessageStreamProps, h: &Handle<Self>) -> Self {
        let url = messages_href(props.limit);
        h.queue_task(move |this, h| {
            #[cfg(target_arch = "wasm32")]
            {
                this._connection = browser::Connection::open(&url, h);
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (this, h, &url);
        });
        MessageStream {
            messages: Vec::new(),
            connected: false,
            #[cfg(target_arch = "wasm32")]
            _connection: None,
        }
    }

    fn render(&self, _: &MessageStreamProps, _: &Handle<Self>) -> Node {
        let connected = self.connected;
        let status = if connected {
            css("display: flex; align-items: center; gap: 0.5rem; padding: 0.75rem 1rem; background: #e3f2fd; border-left: 4px solid #2196f3; border-radius: 4px; color: #1976d2; font-weight: 500;")
        } else {
            css("display: flex; align-items: center; gap: 0.5rem; padding: 0.75rem 1rem; background: #ffebee; border-left: 4px solid #f44336; border-radius: 4px; color: #c62828; font-weight: 500;")
        };
        let dot = if connected {
            css("width: 10px; height: 10px; border-radius: 50%; background: #2196f3; animation: pulse 2s ease-in-out infinite;")
        } else {
            css("width: 10px; height: 10px; border-radius: 50%; background: #f44336; animation: none;")
        };
        let list = if self.messages.is_empty() {
            rsx! { <li mix={css("text-align: center; color: #999; padding: 3rem 1rem;")}>"Waiting for messages..."</li> }
        } else {
            self.messages
                .iter()
                .map(|m| rsx! {
                    <li key={m.count} class="message" mix={css("padding: 0.75rem 1rem; background: #f8f9fa; border-left: 3px solid #4caf50; border-radius: 4px; margin-bottom: 0.5rem; animation: slideIn 0.3s ease-out;")}>
                        <span mix={css("font-weight: 600; color: #4caf50;")}>{format!("#{}", m.count)}</span>
                        " "
                        <span mix={css("color: #666;")}>{m.message.clone()}</span>
                    </li>
                })
                .collect::<Vec<_>>()
                .into_node()
        };
        rsx! {
            <>
                <div mix={card_style()}>
                    <div id="connection" mix={status}>
                        <span mix={dot}></span>
                        <span>{if connected { "Connected" } else { "Disconnected" }}</span>
                    </div>
                </div>
                <div mix={[card_style(), css("min-height: 200px; margin-bottom: 0;")]}>
                    <h2 mix={css("color: #333; margin-bottom: 1rem; font-size: 1.25rem;")}>"Messages"</h2>
                    <ul mix={css("list-style: none;")}>{list}</ul>
                </div>
            </>
        }
    }
}

pub fn card_style() -> rmx::Mixin {
    css("background: white; padding: 1.5rem; border-radius: 8px; box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1); margin-bottom: 1.5rem;")
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::{EventSource, MessageEvent};

    use super::{Message, MessageStream};
    use rmx::Handle;

    /// An open `EventSource` and its listeners. Dropping it closes the connection.
    pub(crate) struct Connection {
        source: EventSource,
        _listeners: Vec<Closure<dyn Fn(web_sys::Event)>>,
    }

    impl Connection {
        pub(crate) fn open(url: &str, h: &Handle<MessageStream>) -> Option<Connection> {
            let source = EventSource::new(url).ok()?;
            let on_open = {
                let h = h.clone();
                Closure::<dyn Fn(web_sys::Event)>::new(move |_| {
                    h.with(|this, h| {
                        this.connected = true;
                        h.update();
                    });
                })
            };
            let on_message = {
                let h = h.clone();
                Closure::<dyn Fn(web_sys::Event)>::new(move |event: web_sys::Event| {
                    let Some(data) = event.dyn_ref::<MessageEvent>().and_then(|e| e.data().as_string()) else { return };
                    let Ok(message) = serde_json::from_str::<Message>(&data) else { return };
                    h.with(|this, h| {
                        this.messages.push(message);
                        h.update();
                    });
                })
            };
            let on_error = {
                let (h, source) = (h.clone(), source.clone());
                Closure::<dyn Fn(web_sys::Event)>::new(move |_| {
                    source.close();
                    h.with(|this, h| {
                        this.connected = false;
                        h.update();
                    });
                })
            };
            for (name, listener) in [("open", &on_open), ("message", &on_message), ("error", &on_error)] {
                let _ = source.add_event_listener_with_callback(name, listener.as_ref().unchecked_ref());
            }
            // Close as soon as the component leaves the page.
            let closer = source.clone();
            let on_abort = Closure::once_into_js(move || closer.close());
            let _ = h.signal().add_event_listener_with_callback("abort", on_abort.unchecked_ref());
            Some(Connection { source, _listeners: vec![on_open, on_message, on_error] })
        }
    }

    impl Drop for Connection {
        fn drop(&mut self) {
            self.source.close();
        }
    }
}

/// Browser entry: hydrate the client entries and start soft navigation.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    rmx::run(rmx::RunOptions { entries: vec![rmx::entry::<MessageStream>()], ..Default::default() });
}
