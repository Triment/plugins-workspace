// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use serde::de::DeserializeOwned;
use tauri::{
    plugin::{Builder, PluginApi, TauriPlugin},
    AppHandle, Manager, Runtime,
};

mod commands;
mod config;
mod error;

pub use error::{Error, Result};

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "app.tauri.deep_link";

fn init_deep_link<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<DeepLink<R>> {
    #[cfg(target_os = "android")]
    {
        use tauri::ipc::{Channel, InvokeBody};

        let handle = _api.register_android_plugin(PLUGIN_IDENTIFIER, "DeepLinkPlugin")?;

        let app_handle = app.clone();
        handle.run_mobile_plugin::<()>(
            "setEventHandler",
            imp::EventHandler {
                handler: Channel::new(move |event| {
                    println!("got channel event: {:?}", &event);

                    let url = match event {
                        InvokeBody::Json(payload) => payload
                            .get("url")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_owned()),
                        _ => None,
                    };

                    let payload = vec![url];
                    app_handle.trigger_global(
                        "deep-link://new-url",
                        Some(serde_json::to_string(&payload).unwrap()),
                    );
                    let _ = app_handle.emit_all("deep-link://new-url", payload);
                    Ok(())
                }),
            },
        )?;

        return Ok(DeepLink(handle));
    }

    #[cfg(not(target_os = "android"))]
    Ok(DeepLink {
        app: app.clone(),
        current: Default::default(),
    })
}

#[cfg(target_os = "android")]
mod imp {
    use tauri::{plugin::PluginHandle, Runtime};

    use serde::{Deserialize, Serialize};
    use tauri::ipc::Channel;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct EventHandler {
        pub handler: Channel,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetCurrentResponse {
        pub url: Option<url::Url>,
    }

    /// Access to the deep-link APIs.
    pub struct DeepLink<R: Runtime>(pub(crate) PluginHandle<R>);

    impl<R: Runtime> DeepLink<R> {
        /// Get the current URLs that triggered the deep link.
        pub fn get_current(&self) -> crate::Result<Option<Vec<url::Url>>> {
            self.0
                .run_mobile_plugin::<GetCurrentResponse>("getCurrent", ())
                .map(|v| v.url.map(|url| vec![url]))
                .map_err(Into::into)
        }
    }
}

#[cfg(not(target_os = "android"))]
mod imp {
    use std::{path::Path, sync::Mutex};
    use tauri::{AppHandle, Runtime};
    #[cfg(windows)]
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    /// Access to the deep-link APIs.
    pub struct DeepLink<R: Runtime> {
        #[allow(dead_code)]
        pub(crate) app: AppHandle<R>,
        pub(crate) current: Mutex<Option<Vec<url::Url>>>,
    }

    impl<R: Runtime> DeepLink<R> {
        /// Get the current URLs that triggered the deep link.
        ///
        /// ## Platform-specific:
        ///
        /// -**Windows / Linux**: Unsupported.
        pub fn get_current(&self) -> crate::Result<Option<Vec<url::Url>>> {
            Ok(self.current.lock().unwrap().clone())
        }

        /// Register the app as the default handler for the specified protocol.
        ///
        /// - `protocol`: The name of the protocol without `://`. For example, if you want your app to handle `tauri://` links, call this method with `tauri` as the protocol.
        ///
        /// ## Platform-specific:
        ///
        /// -**macOS / Android / iOS**: Unsupported.
        pub fn register<S: AsRef<str>>(&self, protocol: S) -> crate::Result<()> {
            #[cfg(windows)]
            {
                let hkcu = RegKey::predef(HKEY_CURRENT_USER);
                let base = Path::new("Software")
                    .join("Classes")
                    .join(protocol.as_ref());

                let exe = tauri::utils::platform::current_exe()?
                    .display()
                    .to_string()
                    .replace("\\\\?\\", "");

                let (key, _) = hkcu.create_subkey(&base)?;
                key.set_value(
                    "",
                    &format!("URL:{} protocol", self.app.config().tauri.bundle.identifier),
                )?;
                key.set_value("URL Protocol", &"")?;

                let (icon, _) = hkcu.create_subkey(base.join("DefaultIcon"))?;
                icon.set_value("", &format!("{},0", &exe))?;

                let (cmd, _) =
                    hkcu.create_subkey(base.join("shell").join("open").join("command"))?;

                cmd.set_value("", &format!("{} \"%1\"", &exe))?;
            }

            #[cfg(target_os = "linux")]
            {
                // TODO: linux
            }

            Ok(())
        }

        /// Unregister the app as the default handler for the specified protocol.
        ///
        /// - `protocol`: The name of the protocol without `://`.
        ///
        /// ## Platform-specific:
        ///
        /// -**macOS / Android / iOS**: Unsupported.
        pub fn unregister<S: AsRef<str>>(&self, protocol: S) -> crate::Result<()> {
            #[cfg(windows)]
            {
                let hkcu = RegKey::predef(HKEY_CURRENT_USER);
                let base = Path::new("Software")
                    .join("Classes")
                    .join(protocol.as_ref());

                hkcu.delete_subkey_all(base)?;
            }
            #[cfg(target_os = "linux")]
            {
                // TODO: linux
            }

            Ok(())
        }

        /// Check whether the app is the default handler for the specified protocol.
        ///
        /// - `protocol`: The name of the protocol without `://`.
        ///
        /// ## Platform-specific:
        ///
        /// -**macOS / Android / iOS**: Unsupported, always returns `Ok(false)`
        pub fn is_registered<S: AsRef<str>>(&self, protocol: S) -> crate::Result<bool> {
            #[cfg(windows)]
            {
                let hkcu = RegKey::predef(HKEY_CURRENT_USER);

                let cmd_reg = hkcu.open_subkey(format!(
                    "Software\\Classes\\{}\\shell\\open\\command",
                    protocol.as_ref()
                ))?;

                let registered_cmd: String = cmd_reg.get_value("")?;

                let exe = tauri::utils::platform::current_exe()?
                    .display()
                    .to_string()
                    .replace("\\\\?\\", "");

                return Ok(registered_cmd == format!("{} \"%1\"", &exe));
            }
            #[cfg(target_os = "linux")]
            {
                // TODO: linux
                return Ok(false);
            }

            #[cfg(not(any(windows, target_os = "linux")))]
            Ok(false)
        }
    }
}

pub use imp::DeepLink;

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to access the deep-link APIs.
pub trait DeepLinkExt<R: Runtime> {
    fn deep_link(&self) -> &DeepLink<R>;
}

impl<R: Runtime, T: Manager<R>> crate::DeepLinkExt<R> for T {
    fn deep_link(&self) -> &DeepLink<R> {
        self.state::<DeepLink<R>>().inner()
    }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R, Option<config::Config>> {
    Builder::new("deep-link")
        .js_init_script(include_str!("api-iife.js").to_string())
        .invoke_handler(tauri::generate_handler![commands::get_current])
        .setup(|app, api| {
            app.manage(init_deep_link(app, api)?);
            Ok(())
        })
        .on_event(|_app, _event| {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            if let tauri::RunEvent::Opened { urls } = _event {
                let _ = _app.emit("deep-link://new-url", urls);
                _app.state::<DeepLink<R>>()
                    .current
                    .lock()
                    .unwrap()
                    .replace(urls.clone());
            }
        })
        .build()
}
