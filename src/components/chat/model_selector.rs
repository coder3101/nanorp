use leptos::prelude::*;
use uuid::Uuid;
use crate::components::ui::dropdown_menu::{
    DropdownMenu, DropdownMenuTrigger, DropdownMenuContent,
    DropdownMenuItem, DropdownMenuSeparator, DropdownMenuLabel,
};
use crate::models::provider::{ModelInfo, Provider};
use crate::server::provider::{list_provider_models, list_providers};

/// A provider paired with the result of fetching its models.
type ProviderModels = (Provider, Result<Vec<ModelInfo>, String>);

async fn load_providers_with_models() -> Vec<ProviderModels> {
    let providers = match list_providers().await {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for p in providers {
        let models = list_provider_models(p.id).await.map_err(|e| e.to_string());
        out.push((p, models));
    }
    out
}

#[component]
pub fn ModelSelector(
    selected_provider: RwSignal<Option<Uuid>>,
    selected_model: RwSignal<Option<String>>,
    /// Optional preferred model id from settings (used for the default pick).
    #[prop(optional, into)] preferred_model: MaybeProp<String>,
) -> impl IntoView {
    // Client-only resource — result isn't serializable and only matters after
    // hydration, so `LocalResource` is the right fit.
    let data = LocalResource::new(load_providers_with_models);

    // Auto-select a sensible default once data is available and nothing is set.
    Effect::new(move |_| {
        if selected_model.get().is_some() {
            return;
        }
        let Some(list) = data.get() else { return };
        if list.is_empty() {
            return;
        }

        // Prefer the default provider, else the first one.
        let Some((provider, models)) = list
            .iter()
            .find(|(p, _)| p.is_default)
            .or_else(|| list.first())
            .cloned()
            .map(|(p, m)| (p, m.unwrap_or_default()))
        else {
            return;
        };

        if models.is_empty() {
            return;
        }

        // Prefer the settings model if the provider actually offers it.
        let preferred = preferred_model.get();
        let chosen = preferred
            .as_ref()
            .filter(|pm| models.iter().any(|m| &m.id == *pm))
            .cloned()
            .unwrap_or_else(|| models[0].id.clone());

        selected_provider.set(Some(provider.id));
        selected_model.set(Some(chosen));
    });

    let display_text = move || {
        selected_model.get().unwrap_or_else(|| "Select model".to_string())
    };

    let content = move || {
        let list = data.get().unwrap_or_default();
        let mut items: Vec<AnyView> = Vec::new();

        if list.is_empty() {
            items.push(view! {
                <DropdownMenuLabel>"No providers configured"</DropdownMenuLabel>
            }.into_any());
            items.push(view! {
                <DropdownMenuItem>
                    <leptos_router::components::A href="/settings" attr:class="no-underline">
                        "Go to settings"
                    </leptos_router::components::A>
                </DropdownMenuItem>
            }.into_any());
            return items;
        }

        for (provider, models) in list.into_iter() {
            let provider_id = provider.id;
            items.push(view! {
                <DropdownMenuLabel>{provider.name.clone()}</DropdownMenuLabel>
            }.into_any());

            match models {
                Ok(models) if models.is_empty() => {
                    items.push(view! {
                        <div class="px-2 py-1.5 text-xs text-muted-foreground">"No models found"</div>
                    }.into_any());
                }
                Ok(models) => {
                    for model in models {
                        let model_id = model.id.clone();
                        let model_name = model.name.clone();
                        let sel = selected_model;
                        let is_selected =
                            Signal::derive(move || sel.get().as_deref() == Some(model_id.as_str()));
                        let pick_id = model.id.clone();
                        items.push(view! {
                            <DropdownMenuItem
                                on_select=Callback::new(move |_| {
                                    selected_provider.set(Some(provider_id));
                                    selected_model.set(Some(pick_id.clone()));
                                    // Remember this choice as the default for
                                    // future chats. Best-effort: a failure here
                                    // only means the preference isn't saved.
                                    let model = pick_id.clone();
                                    leptos::task::spawn_local(async move {
                                        if let Ok(mut s) = crate::server::settings::get_settings().await {
                                            if s.default_model.as_deref() != Some(model.as_str())
                                                || s.default_provider_id != Some(provider_id)
                                            {
                                                s.default_model = Some(model);
                                                s.default_provider_id = Some(provider_id);
                                                let _ = crate::server::settings::update_settings(s).await;
                                            }
                                        }
                                    });
                                })
                            >
                                <span class="flex w-full items-center gap-2">
                                    <span class="w-3.5 shrink-0 text-primary">
                                        {move || if is_selected.get() { "✓" } else { "" }}
                                    </span>
                                    <span class="truncate">{model_name}</span>
                                </span>
                            </DropdownMenuItem>
                        }.into_any());
                    }
                }
                Err(e) => {
                    items.push(view! {
                        <div class="px-2 py-1.5 text-xs text-destructive">
                            "⚠ " {e}
                        </div>
                    }.into_any());
                }
            }

            items.push(view! { <DropdownMenuSeparator /> }.into_any());
        }

        items.push(view! {
            <DropdownMenuItem>
                <leptos_router::components::A href="/settings" attr:class="no-underline text-muted-foreground">
                    "Manage providers..."
                </leptos_router::components::A>
            </DropdownMenuItem>
        }.into_any());

        items
    };

    let has_model = Signal::derive(move || selected_model.get().is_some());

    view! {
        <DropdownMenu>
            <DropdownMenuTrigger class="inline-flex items-center justify-center rounded-lg text-sm font-medium \
                                         transition-colors focus-visible:outline-none focus-visible:ring-2 \
                                         focus-visible:ring-ring border border-input bg-background shadow-sm \
                                         hover:bg-accent hover:text-accent-foreground h-8 max-w-[220px] px-3">
                <span class="flex items-center gap-2 overflow-hidden">
                    <span class=move || {
                        let base = "h-1.5 w-1.5 rounded-full shrink-0";
                        if has_model.get() {
                            format!("{base} bg-green-500")
                        } else {
                            format!("{base} bg-muted-foreground/40")
                        }
                    } />
                    <span class="truncate font-medium">{display_text}</span>
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24"
                         fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
                         stroke-linejoin="round" class="shrink-0 text-muted-foreground">
                        <path d="m6 9 6 6 6-6"/>
                    </svg>
                </span>
            </DropdownMenuTrigger>
            <DropdownMenuContent align=crate::components::ui::dropdown_menu::DropdownAlign::End class="max-h-80 overflow-y-auto scroll-area">
                {content}
            </DropdownMenuContent>
        </DropdownMenu>
    }
}
