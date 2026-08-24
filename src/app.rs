use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::components::{ParentRoute, Route, Router, Routes};
use leptos_router::path;

use crate::api::auth::{current_user, Login, Logout, Signup};
use crate::components::nav::Nav;
use crate::models::AuthUser;
use crate::pages::item_detail::ItemDetailPage;
use crate::pages::login::LoginPage;
use crate::pages::signup::SignupPage;
use crate::pages::wishlist::WishlistPage;

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <link rel="icon" href="/favicon.ico"/>
                <AutoReload options=options.clone() />
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

/// Who is signed in. Shared through context so the nav, the route guard and the pages all
/// read one source of truth rather than each firing their own request.
pub type UserResource = Resource<Result<Option<AuthUser>, ServerFnError>>;

/// The auth actions live in `App` rather than in the pages that submit them, because the
/// user resource has to refetch when any of them completes.
#[derive(Clone, Copy)]
pub struct AuthActions {
    pub login: ServerAction<Login>,
    pub signup: ServerAction<Signup>,
    pub logout: ServerAction<Logout>,
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    let login = ServerAction::<Login>::new();
    let signup = ServerAction::<Signup>::new();
    let logout = ServerAction::<Logout>::new();
    provide_context(AuthActions {
        login,
        signup,
        logout,
    });

    // Re-resolving on every auth action is what makes the nav and the guard react to a
    // sign-in or sign-out without a full page load.
    let user: UserResource = Resource::new(
        move || {
            (
                login.version().get(),
                signup.version().get(),
                logout.version().get(),
            )
        },
        |_| async move { current_user().await },
    );
    provide_context(user);

    view! {
        <Stylesheet id="leptos" href="/pkg/pricey.css"/>
        <Title text="pricey"/>

        <Router>
            <Nav/>
            <main>
                <Routes fallback=|| view! { <p class="empty">"Page not found."</p> }>
                    <Route path=path!("/login") view=LoginPage/>
                    <Route path=path!("/signup") view=SignupPage/>
                    <ParentRoute path=path!("") view=RequireAuth>
                        <Route path=path!("") view=WishlistPage/>
                        <Route path=path!("/items/:id") view=ItemDetailPage/>
                    </ParentRoute>
                </Routes>
            </main>
        </Router>
    }
}

/// Client-side gate for the signed-in pages.
///
/// This is convenience, not security: it stops a signed-out visitor seeing an empty shell
/// and a burst of failing requests. Every server function enforces the same rule itself.
#[component]
fn RequireAuth() -> impl IntoView {
    use leptos_router::components::{Outlet, Redirect};

    let user = expect_context::<UserResource>();
    // The `<Outlet/>` below is built inside a suspended future, which runs under an owner
    // nested in the `<Suspense/>`. Restoring this one first keeps the child routes on the
    // same owner they would have had otherwise -- the same thing `<ProtectedParentRoute/>`
    // does upstream.
    let owner = Owner::current().expect("RequireAuth rendered without an owner");

    view! {
        <Suspense fallback=|| view! { <p class="loading">"Loading..."</p> }>
            // Awaited rather than read with `.get()`. A resource keeps its previous answer
            // while it re-runs, and signing in is exactly what makes this one re-run: a
            // synchronous read here would still see the "nobody is signed in" from before
            // the login and bounce the visitor straight back to the form they just
            // submitted. Awaiting waits for the answer that accounts for the new session.
            {move || {
                let owner = owner.clone();
                Suspend::new(async move {
                    match user.await {
                        Ok(Some(_)) => owner.with(|| view! { <Outlet/> }.into_any()),
                        Ok(None) => view! { <Redirect path="/login"/> }.into_any(),
                        Err(_) => view! { <Redirect path="/login"/> }.into_any(),
                    }
                })
            }}
        </Suspense>
    }
}
