use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use crate::api::auth::{ResendVerification, UNVERIFIED_MESSAGE};
use crate::app::AuthActions;
use crate::components::action_error;

#[component]
pub fn LoginPage() -> impl IntoView {
    let action = expect_context::<AuthActions>().login;
    let resend = ServerAction::<ResendVerification>::new();
    let query = use_query_map();

    let error = move || action_error(action.value().get());
    // The verify route redirects here with the outcome of the emailed link.
    let verify_notice = move || {
        query
            .read()
            .get("verify")
            .map(|status| match status.as_str() {
                "verified" => ("notice", "Your email is confirmed. Sign in to continue."),
                "expired" => ("error", "That link has expired. Request a new one below."),
                "used" => ("notice", "That link was already used. Try signing in."),
                "missing" | "invalid" => ("error", "That verification link is not valid."),
                _ => ("error", "Something went wrong confirming your email."),
            })
    };

    // Only offered when it is actually the problem, so the form is not noise for everyone.
    let needs_verification = move || {
        error().is_some_and(|e| e == UNVERIFIED_MESSAGE)
            || verify_notice().is_some_and(|(_, m)| m.contains("expired"))
    };
    let resent = move || matches!(resend.value().get(), Some(Ok(())));

    view! {
        <section class="auth-page">
            <h1>"Sign in"</h1>

            <Show when=move || verify_notice().is_some()>
                {move || verify_notice().map(|(kind, message)| {
                    view! { <p class=kind role="status">{message}</p> }
                })}
            </Show>

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

            <Show when=needs_verification>
                <div class="resend-box">
                    <Show
                        when=resent
                        fallback=move || view! {
                            <p class="muted">"Need a new confirmation link?"</p>
                            <ActionForm action=resend attr:class="stack">
                                <label>
                                    "Email"
                                    <input type="email" name="email" autocomplete="email" required/>
                                </label>
                                <button
                                    type="submit"
                                    class="secondary"
                                    disabled=move || resend.pending().get()
                                >
                                    {move || if resend.pending().get() {
                                        "Sending..."
                                    } else {
                                        "Send a new link"
                                    }}
                                </button>
                            </ActionForm>
                        }
                    >
                        // Worded so it reveals nothing about whether the address is registered.
                        <p class="notice">
                            "If that address needs confirming, a new link is on its way."
                        </p>
                    </Show>
                </div>
            </Show>

            <p class="muted">"No account yet? " <a href="/signup">"Create one"</a></p>
        </section>
    }
}
