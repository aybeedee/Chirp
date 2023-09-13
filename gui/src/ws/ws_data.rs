mod imp {
    use adw::prelude::*;
    use adw::subclass::prelude::*;
    use gio::glib::Sender;
    use glib::once_cell::sync::Lazy;
    use glib::subclass::Signal;
    use glib::{derived_properties, object_subclass, Properties, SignalHandlerId};
    use gtk::glib;
    use soup::WebsocketConnection;
    use std::cell::{Cell, OnceCell, RefCell};

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::WSObject)]
    pub struct WSObject {
        #[property(get, set)]
        pub ws_conn: RefCell<Option<WebsocketConnection>>,
        #[property(get, set)]
        pub ws_id: Cell<u64>,
        pub ws_signal_id: RefCell<Option<SignalHandlerId>>,
        pub ws_sender: OnceCell<Sender<Option<WebsocketConnection>>>,
        pub notifier: OnceCell<Sender<bool>>,
        #[property(get, set)]
        pub reconnecting: Cell<bool>,
    }

    #[object_subclass]
    impl ObjectSubclass for WSObject {
        const NAME: &'static str = "WSObject";
        type Type = super::WSObject;
    }

    #[derived_properties]
    impl ObjectImpl for WSObject {
        fn signals() -> &'static [Signal] {
            static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
                vec![
                    Signal::builder("ws-success")
                        .param_types([bool::static_type()])
                        .build(),
                    Signal::builder("ws-reconnect")
                        .param_types([bool::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }
}

use adw::subclass::prelude::*;
use gio::Cancellable;
use glib::{
    clone, timeout_add_seconds_local_once, wrapper, ControlFlow, MainContext, Object, Priority,
    SignalHandlerId,
};
use gtk::{glib, prelude::*};
use soup::{prelude::*, Message, Session};
use tracing::{error, info};

wrapper! {
    pub struct WSObject(ObjectSubclass<imp::WSObject>);
}

impl WSObject {
    pub fn new() -> Self {
        let obj: WSObject = Object::builder().build();
        obj.set_ws();
        obj
    }

    pub fn connect_to_ws(&self) {
        let session = Session::new();
        let sender = self.imp().ws_sender.get().unwrap().clone();

        let websocket_url = "ws://127.0.0.1:8080/ws/";

        let message = Message::new("GET", websocket_url).unwrap();
        let cancel = Cancellable::new();

        let is_reconnecting = self.reconnecting();
        let notifier = self.imp().notifier.get().unwrap().clone();

        info!("Starting websocket connection with {}", websocket_url);
        session.websocket_connect_async(
            &message,
            None,
            &[],
            Priority::default(),
            Some(&cancel),
            move |result| match result {
                Ok(connection) => {
                    sender.send(Some(connection)).unwrap();
                    if is_reconnecting {
                        notifier.send(true).unwrap()
                    };
                }
                Err(error) => {
                    sender.send(None).unwrap();
                    error!("WebSocket connection error: {:?}", error);
                }
            },
        );
    }

    fn set_ws(&self) {
        let (sender, receiver) = MainContext::channel(Priority::DEFAULT);

        self.imp().ws_sender.set(sender).unwrap();

        let (notifier_send, notifier_receive) = MainContext::channel(Priority::DEFAULT);

        self.imp().notifier.set(notifier_send).unwrap();

        self.set_reconnecting(false);

        self.connect_to_ws();

        receiver.attach(
            None,
            clone!(@weak self as ws_object => @default-return ControlFlow::Break, move |conn| {
                if conn.is_some() {
                    ws_object.set_ws_conn(conn.unwrap());
                    info!("WebSocket connection success");
                    ws_object.emit_by_name::<()>("ws-success", &[&true]);
                    ws_object.start_pinging();
                } else {
                    error!("WebSocket connection failed. Starting again");
                    timeout_add_seconds_local_once(10, move || {
                        ws_object.connect_to_ws();
                    });
                }
                ControlFlow::Continue
            }),
        );

        notifier_receive.attach(
            None,
            clone!(@weak self as ws => @default-return ControlFlow::Break, move |_| {
                ws.emit_by_name::<()>("ws-reconnect", &[&true]);
                info!("Emitted");
                ControlFlow::Continue
            }),
        );
    }

    pub fn send_text_message(&self, message: &str) {
        info!("Sending message to ws: {message}");
        self.ws_conn()
            .unwrap()
            .send_text(&format!("/message {}", message));
    }

    pub fn create_new_user(&self, user_data: String) {
        info!("Connecting to WS to create a new user");
        self.ws_conn()
            .unwrap()
            .send_text(&format!("/create-new-user {}", user_data));
    }

    pub fn update_chatting_with(&self, id: &u64) {
        info!("Sending request for updating chatting with id to {}", id);
        self.ws_conn()
            .unwrap()
            .send_text(&format!("/update-chatting-with {}", id))
    }

    pub fn get_user_data(&self, id: &u64) {
        info!(
            "Sending request for getting UserObject Data with id: {}",
            id
        );
        self.ws_conn()
            .unwrap()
            .send_text(&format!("/get-user-data {}", id))
    }

    pub fn update_ids(&self, id: u64, client_id: u64) {
        info!("Sending info to update ids");
        self.ws_conn()
            .unwrap()
            .send_text(&format!("/update-ids {} {}", id, client_id))
    }

    pub fn image_link_updated(&self, link: &str) {
        info!("Sending updated image link: {link}");
        self.ws_conn()
            .unwrap()
            .send_text(&format!("/image-updated {}", link))
    }

    pub fn name_updated(&self, name: &str) {
        info!("Sending updated name: {name}");
        self.ws_conn()
            .unwrap()
            .send_text(&format!("/name-updated {}", name))
    }

    pub fn start_pinging(&self) {
        let conn = self.ws_conn().unwrap();
        conn.set_keepalive_interval(5);

        conn.connect_closed(clone!(@weak self as ws => move |_| {
            info!("connection closed. Starting again");
            ws.imp().ws_conn.replace(None);
            ws.set_reconnecting(true);
            ws.connect_to_ws();
        }));
    }

    pub fn reconnect_user(&self, owner_id: u64, user_data: String) {
        info!("Updating WS to reconnect old owner: {}", owner_id);
        self.ws_conn()
            .unwrap()
            .send_text(&format!("/reconnect-user {} {user_data}", owner_id))
    }

    pub fn set_signal_id(&self, id: SignalHandlerId) {
        self.imp().ws_signal_id.replace(Some(id));
    }
}
