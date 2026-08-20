//! The persistent header.

use leptos::prelude::*;

use crate::app::{AuthActions, UserResource};

#[component]
pub fn Nav() -> impl IntoView {
    let auth = expect_context::<AuthActions>();
    let user = expect_context::<UserResource>();

    view! {
        <header class="nav">
            <a class="brand" href="/">"pricey"</a>

            <Transition>
                {move || {
                    user.get()
                        .and_then(Result::ok)
                        .flatten()
                        .map(|signed_in| {
                            view! {
                                <div class="nav-user">
                                    <span class="nav-email">{signed_in.email}</span>
                                    <ActionForm action=auth.logout>
                                        <button type="submit" class="linklike">"Sign out"</button>
                                    </ActionForm>
                                </div>
                            }
                        })
                }}
            </Transition>
        </header>
    }
}
