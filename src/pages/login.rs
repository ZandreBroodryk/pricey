use leptos::prelude::*;

use crate::app::AuthActions;
use crate::components::action_error;

#[component]
pub fn LoginPage() -> impl IntoView {
    let action = expect_context::<AuthActions>().login;
    let error = move || action_error(action.value().get());

    view! {
        <section class="auth-page">
            <h1>"Sign in"</h1>

            <ActionForm action=action attr:class="stack">
                <label>
                    "Email"
                    <input type="email" name="email" autocomplete="email" required/>
                </label>
                <label>
                    "Password"
                    <input
                        type="password"
                        name="password"
                        autocomplete="current-password"
                        required
                    />
                </label>
                <button type="submit" disabled=move || action.pending().get()>
                    {move || if action.pending().get() { "Signing in..." } else { "Sign in" }}
                </button>
            </ActionForm>

            <Show when=move || error().is_some()>
                <p class="error" role="alert">{error}</p>
            </Show>

            <p class="muted">"No account yet? " <a href="/signup">"Create one"</a></p>
        </section>
    }
}
