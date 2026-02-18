use dioxus::prelude::*;

use crate::templates::render_to_html;

#[allow(non_snake_case)]
#[component]
fn PhoneForm(error: Option<String>) -> Element {
    rsx! {
        head {
            meta { charset: "utf-8" }
            meta { name: "viewport", content: "width=device-width, initial-scale=1" }
            title { "Login — Root Signal" }
            script { src: "https://cdn.tailwindcss.com" }
        }
        body { class: "flex items-center justify-center min-h-screen bg-gray-50 font-sans text-gray-900",
            div { class: "w-full max-w-sm bg-white border border-gray-200 rounded-lg p-8",
                h2 { class: "text-xl font-semibold mb-1", "Admin Login" }
                p { class: "text-gray-500 text-sm mb-4",
                    "Enter your phone number to receive a verification code."
                }
                if let Some(err) = &error {
                    div { class: "bg-red-50 border border-red-200 text-red-800 text-sm px-3 py-2 rounded mb-4",
                        "{err}"
                    }
                }
                form { method: "POST", action: "/admin/login",
                    label { r#for: "phone", class: "block text-sm text-gray-500 mb-1",
                        "Phone Number (E.164)"
                    }
                    input {
                        r#type: "tel", name: "phone", id: "phone", required: true,
                        placeholder: "+15551234567",
                        class: "w-full px-3 py-2.5 border border-gray-300 rounded text-base mb-3",
                        autofocus: true
                    }
                    button {
                        r#type: "submit",
                        class: "w-full py-2.5 bg-blue-600 text-white rounded text-sm font-medium cursor-pointer hover:bg-blue-800",
                        "Send Code"
                    }
                }
            }
        }
    }
}

#[allow(non_snake_case)]
#[component]
fn VerifyForm(phone: String, error: Option<String>) -> Element {
    let masked = mask_phone(&phone);
    rsx! {
        head {
            meta { charset: "utf-8" }
            meta { name: "viewport", content: "width=device-width, initial-scale=1" }
            title { "Verify — Root Signal" }
            script { src: "https://cdn.tailwindcss.com" }
        }
        body { class: "flex items-center justify-center min-h-screen bg-gray-50 font-sans text-gray-900",
            div { class: "w-full max-w-sm bg-white border border-gray-200 rounded-lg p-8",
                h2 { class: "text-xl font-semibold mb-1", "Enter Code" }
                p { class: "text-gray-500 text-sm mb-4",
                    "We sent a code to {masked}"
                }
                if let Some(err) = &error {
                    div { class: "bg-red-50 border border-red-200 text-red-800 text-sm px-3 py-2 rounded mb-4",
                        "{err}"
                    }
                }
                form { method: "POST", action: "/admin/verify",
                    input { r#type: "hidden", name: "phone", value: "{phone}" }
                    label { r#for: "code", class: "block text-sm text-gray-500 mb-1",
                        "Verification Code"
                    }
                    input {
                        r#type: "text", name: "code", id: "code", required: true,
                        inputmode: "numeric", autocomplete: "one-time-code",
                        maxlength: "6", pattern: "[0-9]*",
                        placeholder: "123456",
                        class: "w-full px-3 py-2.5 border border-gray-300 rounded text-2xl tracking-[8px] text-center mb-3",
                        autofocus: true
                    }
                    button {
                        r#type: "submit",
                        class: "w-full py-2.5 bg-blue-600 text-white rounded text-sm font-medium cursor-pointer hover:bg-blue-800",
                        "Verify"
                    }
                }
            }
        }
    }
}

fn mask_phone(phone: &str) -> String {
    if phone.len() > 4 {
        let visible = &phone[phone.len() - 4..];
        format!("***{visible}")
    } else {
        "****".to_string()
    }
}

pub fn render_login(error: Option<String>) -> String {
    let mut dom = VirtualDom::new_with_props(PhoneForm, PhoneFormProps { error });
    dom.rebuild_in_place();
    render_to_html(&dom)
}

pub fn render_verify(phone: String, error: Option<String>) -> String {
    let mut dom = VirtualDom::new_with_props(VerifyForm, VerifyFormProps { phone, error });
    dom.rebuild_in_place();
    render_to_html(&dom)
}
