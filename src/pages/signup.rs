use leptos::prelude::*;

use crate::api::auth::MIN_PASSWORD_LEN;
use crate::app::AuthActions;
use crate::components::action_error;

#[component]
pub fn SignupPage() -> impl IntoView {
    let action = expect_context::<AuthActions>().signup;
    let error = move || action_error(action.value().get());
    // Signup no longer signs you in -- it sends a link -- so success is a state this page
    // renders rather than a redirect.
    let sent = move || matches!(action.value().get(), Some(Ok(())));

    view! {
        <section class="auth-page">
            <Show
                when=sent
                fallback=move || view! {
                    <h1>"Create an account"</h1>

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
                                autocomplete="new-password"
                                minlength=MIN_PASSWORD_LEN.to_string()
                                required
                            />
                        </label>
                        <label>
                            "Confirm password"
                            <input
                                type="password"
                                name="confirm"
                                autocomplete="new-password"
                                minlength=MIN_PASSWORD_LEN.to_string()
                                required
                            />
                        </label>
                        <button type="submit" disabled=move || action.pending().get()>
                            {move || if action.pending().get() { "Creating..." } else { "Create account" }}
                        </button>
                    </ActionForm>

                    <Show when=move || error().is_some()>
                        <p class="error" role="alert">{error}</p>
                    </Show>

                    <p class="muted">
                        "At least " {MIN_PASSWORD_LEN} " characters. Already registered? "
                        <a href="/login">"Sign in"</a>
                    </p>
                }
            >
                <h1>"Check your email"</h1>
                <p>
                    "We have sent you a link to confirm your address. Click it to finish \
                     setting up your account."
                </p>
                <p class="muted">
                    "The link expires in 24 hours. Nothing arrived? You can "
                    <a href="/login">"request a new one from the sign-in page"</a> "."
                </p>
            </Show>
        </section>
    }
}
