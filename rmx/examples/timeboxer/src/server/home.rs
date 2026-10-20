//! `actions/home`: signed-in visitors go to their latest schedule; others see the intro.

use rmx::{css, rsx, Component, Handle, Node};
use rmx::fetch::{response, Response};
use rmx::router::{Context, Router};

use super::schedules::SchedulePage;
use super::ui::Layout;
use super::{current_user, db, render, token};
use crate::routes::{schedule_href, ROUTES};
use crate::theme::{self, font_size, space, text};

pub fn routes(router: &mut Router) {
    router.route(&ROUTES.home, index);
}

async fn index(ctx: Context) -> Response {
    let Some(user) = current_user(&ctx) else { return render(&ctx, rsx! { <HomePage/> }) };
    let schedules = db(&ctx).list_schedules(user.id);
    if let Some(latest) = schedules.first() {
        return response::redirect(&schedule_href(latest.id));
    }
    render(&ctx, rsx! { <SchedulePage csrf_token={token(&ctx)} schedules={schedules}/> })
}

#[derive(Clone, Default)]
pub struct HomePageProps;

pub struct HomePage;

impl Component for HomePage {
    type Props = HomePageProps;

    fn setup(_: &HomePageProps, _: &Handle<Self>) -> Self {
        HomePage
    }

    fn render(&self, _: &HomePageProps, _: &Handle<Self>) -> Node {
        rsx! {
            <Layout title="Timeboxer">
                <section mix={css(&format!("margin: 0 auto; max-width: 760px; padding: 32px {};", space::XL))}>
                    <p mix={css(&format!("color: {}; font-size: {}; font-weight: 600; letter-spacing: {}; text-transform: uppercase;", text::MUTED, font_size::XS, theme::LETTER_SPACING_WIDE))}>"Timeboxer"</p>
                    <h1 mix={css(&format!("color: {}; font-size: {}; letter-spacing: {}; line-height: {}; margin: 0;", text::PRIMARY, font_size::XXL, theme::LETTER_SPACING_TIGHT, theme::LINE_HEIGHT_TIGHT))}>"Plan your focused work."</h1>
                    <p mix={css(&format!("color: {}; font-size: {}; line-height: {}; max-width: 560px;", text::SECONDARY, font_size::MD, theme::LINE_HEIGHT_RELAXED))}>
                        "Authentication is ready with in-memory users, separate password records, and session-backed sign-in."
                    </p>
                </section>
            </Layout>
        }
    }
}
