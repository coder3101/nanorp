use crate::components::ui::dropdown_menu::{
    DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator,
    DropdownMenuTrigger,
};
use crate::models::provider::{ModelInfo, ProviderDto};
use crate::server::provider::{list_provider_models, list_providers};
use leptos::prelude::*;
use uuid::Uuid;

#[derive(Clone)]
struct ProviderModels {
    provider: ProviderDto,
    models: Result<Vec<ModelInfo>, String>,
}

#[derive(Clone)]
struct ProviderCatalog {
    providers: Result<Vec<ProviderModels>, String>,
    saved_provider: Option<Uuid>,
    saved_model: Option<String>,
}

async fn load_providers_with_models(loading: RwSignal<bool>) -> ProviderCatalog {
    loading.set(true);
    // Settings and the provider list are independent; model discovery for each
    // provider is also concurrent so one slow endpoint does not serialize all.
    let (providers, settings) =
        futures::join!(list_providers(), crate::server::settings::get_settings());
    let (saved_provider, saved_model) = settings
        .ok()
        .map(|s| (s.default_provider_id, s.default_model))
        .unwrap_or((None, None));

    let providers = match providers {
        Ok(providers) => {
            let models =
                futures::future::join_all(providers.into_iter().map(|provider| async move {
                    let result = list_provider_models(provider.id)
                        .await
                        .map_err(|e| e.to_string());
                    ProviderModels {
                        provider,
                        models: result,
                    }
                }))
                .await;
            Ok(models)
        }
        Err(e) => Err(e.to_string()),
    };
    loading.set(false);
    ProviderCatalog {
        providers,
        saved_provider,
        saved_model,
    }
}

fn remember_choice(provider_id: Uuid, model: String) {
    leptos::task::spawn_local(async move {
        if let Ok(mut settings) = crate::server::settings::get_settings().await {
            if settings.default_model.as_deref() != Some(model.as_str())
                || settings.default_provider_id != Some(provider_id)
            {
                settings.default_model = Some(model);
                settings.default_provider_id = Some(provider_id);
                // Selection remains useful if preference persistence fails;
                // the next load will simply fall back to the default provider.
                let _ = crate::server::settings::update_settings(settings).await;
            }
        }
    });
}

#[component]
pub fn ModelSelector(
    selected_provider: RwSignal<Option<Uuid>>,
    selected_model: RwSignal<Option<String>>,
    /// Optional preferred model id from settings (used when no pair is restored).
    #[prop(optional, into)]
    preferred_model: MaybeProp<String>,
) -> impl IntoView {
    let loading = RwSignal::new(false);
    let data = LocalResource::new(move || load_providers_with_models(loading));
    let initialized = RwSignal::new(false);
    let manual_provider = RwSignal::new(Option::<Uuid>::None);
    let manual_model = RwSignal::new(String::new());
    let manual_error = RwSignal::new(Option::<String>::None);

    // Restore the saved provider and model together. If discovery succeeds and
    // proves the saved ID is stale, select a real model from that same provider.
    // If discovery fails/returns nothing, retaining the pair is a valid manual
    // fallback and is preferable to mixing a saved model with another provider.
    Effect::new(move |_| {
        if initialized.get() {
            return;
        }
        let Some(catalog) = data.get() else { return };
        let Ok(list) = &catalog.providers else { return };
        if list.is_empty() {
            return;
        }
        initialized.set(true);

        if selected_provider.get_untracked().is_some() && selected_model.get_untracked().is_some() {
            manual_provider.set(selected_provider.get_untracked());
            return;
        }

        let saved_entry = catalog.saved_provider.and_then(|id| {
            list.iter()
                .find(|entry| entry.provider.id == id)
                .map(|entry| (id, entry))
        });
        let preferred = preferred_model.get_untracked();
        let mut choice: Option<(Uuid, Option<String>)> = None;

        if let Some((provider_id, entry)) = saved_entry {
            let model = match (&entry.models, catalog.saved_model.clone()) {
                (Ok(models), Some(saved)) if !models.is_empty() => {
                    if models.iter().any(|m| m.id == saved) {
                        Some(saved)
                    } else {
                        Some(models[0].id.clone())
                    }
                }
                (Ok(models), None) if !models.is_empty() => Some(models[0].id.clone()),
                (_, saved) => saved, // failed/empty discovery: preserve manual pair
            };
            choice = Some((provider_id, model));
        } else {
            let preferred_provider = list
                .iter()
                .find(|entry| entry.provider.is_default)
                .or_else(|| list.first());
            if let Some(entry) = preferred_provider {
                let model = entry.models.as_ref().ok().and_then(|models| {
                    if models.is_empty() {
                        None
                    } else {
                        Some(
                            preferred
                                .as_ref()
                                .filter(|id| models.iter().any(|m| &m.id == *id))
                                .cloned()
                                .unwrap_or_else(|| models[0].id.clone()),
                        )
                    }
                });
                choice = Some((entry.provider.id, model));
            }

            // If the default cannot provide models, fall back to the first
            // discoverable provider rather than leaving an unusable selection.
            if choice.as_ref().is_some_and(|(_, model)| model.is_none()) {
                if let Some(entry) = list
                    .iter()
                    .find(|entry| entry.models.as_ref().is_ok_and(|models| !models.is_empty()))
                {
                    let models = entry.models.as_ref().unwrap();
                    choice = Some((entry.provider.id, Some(models[0].id.clone())));
                }
            }
        }

        if let Some((provider_id, model)) = choice {
            selected_provider.set(Some(provider_id));
            manual_provider.set(Some(provider_id));
            selected_model.set(model);
        }
    });

    let display_text = move || {
        if let Some(model) = selected_model.get() {
            model
        } else if loading.get() && data.get_untracked().is_none() {
            "Loading models...".to_string()
        } else {
            "Select model".to_string()
        }
    };
    let has_model = Signal::derive(move || selected_model.get().is_some());

    let content = move || {
        let catalog = data.get();
        let mut items: Vec<AnyView> = Vec::new();

        if loading.get() {
            items.push(
                view! {
                    <div class="px-2 py-1.5 text-xs text-muted-foreground" role="status">
                        "Loading model lists…"
                    </div>
                }
                .into_any(),
            );
        }

        match catalog {
            None if !loading.get_untracked() => {
                items.push(
                    view! { <DropdownMenuLabel>"Waiting to load providers…"</DropdownMenuLabel> }
                        .into_any(),
                );
            }
            None => {}
            Some(catalog) => {
                match catalog.providers {
                    Err(error) => {
                        items.push(
                        view! {
                            <div class="max-w-xs space-y-2 px-2 py-1.5 text-xs text-destructive" role="alert">
                                <p class="break-words">"Could not load providers: " {error}</p>
                                <button
                                    type="button"
                                    class="rounded border border-border px-2 py-1 text-foreground hover:bg-accent"
                                    on:click=move |_| data.refetch()
                                >"Retry"</button>
                            </div>
                        }
                        .into_any(),
                    );
                    }
                    Ok(list) if list.is_empty() => {
                        items.push(view! { <DropdownMenuLabel>"No providers configured"</DropdownMenuLabel> }.into_any());
                    }
                    Ok(list) => {
                        for entry in list {
                            let provider = entry.provider;
                            let provider_id = provider.id;
                            items.push(view! { <DropdownMenuLabel>{provider.name.clone()}</DropdownMenuLabel> }.into_any());

                            match entry.models {
                                Ok(models) if models.is_empty() => {
                                    items.push(
                                        view! {
                                            <div class="px-2 py-1.5 text-xs text-muted-foreground">
                                                "No models discovered. Enter a model ID below."
                                            </div>
                                        }
                                        .into_any(),
                                    );
                                }
                                Ok(models) => {
                                    for model in models {
                                        let model_id = model.id.clone();
                                        let sel_model = selected_model;
                                        let sel_provider = selected_provider;
                                        let is_selected = Signal::derive(move || {
                                            sel_provider.get() == Some(provider_id)
                                                && sel_model.get().as_deref()
                                                    == Some(model_id.as_str())
                                        });
                                        let pick_id = model.id.clone();
                                        items.push(view! {
                                        <DropdownMenuItem
                                            on_select=Callback::new(move |_| {
                                                selected_provider.set(Some(provider_id));
                                                selected_model.set(Some(pick_id.clone()));
                                                manual_provider.set(Some(provider_id));
                                                remember_choice(provider_id, pick_id.clone());
                                            })
                                        >
                                            <span class="flex w-full min-w-0 items-center gap-2">
                                                <span class="w-3.5 shrink-0 text-primary">
                                                    {move || if is_selected.get() { "✓" } else { "" }}
                                                </span>
                                                <span class="break-words">{model.name.clone()}</span>
                                            </span>
                                        </DropdownMenuItem>
                                    }.into_any());
                                    }
                                }
                                Err(error) => {
                                    items.push(
                                    view! {
                                        <div class="max-w-xs space-y-2 px-2 py-1.5 text-xs text-destructive" role="alert">
                                            <p class="break-words">"Model discovery failed: " {error}</p>
                                            <button
                                                type="button"
                                                class="rounded border border-border px-2 py-1 text-foreground hover:bg-accent"
                                                on:click=move |_| data.refetch()
                                            >"Retry discovery"</button>
                                            <p class="text-muted-foreground">"You can still use an exact model ID below."</p>
                                        </div>
                                    }
                                    .into_any(),
                                );
                                }
                            }
                            items.push(view! { <DropdownMenuSeparator /> }.into_any());
                        }
                    }
                }
            }
        }

        items.push(
            view! {
                <div class="max-w-xs space-y-2 px-2 py-2" on:keydown=move |ev| ev.stop_propagation()>
                    <p class="text-xs font-medium">"Manual model ID"</p>
                    <p class="text-xs text-muted-foreground">"Use this when discovery is unsupported or incomplete."</p>
                    <select
                        class="min-h-9 w-full rounded-md border border-input bg-background px-2 text-xs"
                        aria-label="Provider for manual model"
                        on:change=move |ev| {
                            manual_provider.set(event_target_value(&ev).parse().ok());
                            manual_error.set(None);
                        }
                    >
                        {move || {
                            let providers = data.get_untracked()
                                .and_then(|catalog| catalog.providers.ok())
                                .unwrap_or_default();
                            providers.into_iter().map(|entry| {
                                let id = entry.provider.id;
                                let name = entry.provider.name;
                                let selected = move || manual_provider.get() == Some(id);
                                view! {
                                    <option value=id.to_string() selected=selected>{name}</option>
                                }
                            }).collect::<Vec<_>>()
                        }}
                    </select>
                    <input
                        class="min-h-9 w-full rounded-md border border-input bg-background px-2 text-xs"
                        placeholder="e.g. llama3:8b"
                        aria-label="Exact model ID"
                        prop:value=move || manual_model.get()
                        on:input=move |ev| {
                            manual_model.set(event_target_value(&ev));
                            manual_error.set(None);
                        }
                    />
                    {move || manual_error.get().map(|error| view! {
                        <p class="break-words text-xs text-destructive" role="alert">{error}</p>
                    })}
                    <button
                        type="button"
                        class="min-h-9 w-full rounded-md border border-border bg-background px-2 text-xs font-medium hover:bg-accent"
                        on:click=move |_| {
                            let model = manual_model.get().trim().to_string();
                            let provider_id = manual_provider.get().or_else(|| selected_provider.get());
                            if model.is_empty() {
                                manual_error.set(Some("Enter an exact model ID.".to_string()));
                                return;
                            }
                            let Some(provider_id) = provider_id else {
                                manual_error.set(Some("Choose a provider first.".to_string()));
                                return;
                            };
                            selected_provider.set(Some(provider_id));
                            selected_model.set(Some(model.clone()));
                            manual_error.set(None);
                            manual_model.set(String::new());
                            remember_choice(provider_id, model);
                        }
                    >"Use model ID"</button>
                </div>
            }
            .into_any(),
        );
        items.push(view! { <DropdownMenuSeparator /> }.into_any());
        items.push(view! {
            <DropdownMenuItem>
                <leptos_router::components::A href="/settings" attr:class="no-underline text-muted-foreground">
                    "Manage providers..."
                </leptos_router::components::A>
            </DropdownMenuItem>
        }.into_any());
        items
    };

    view! {
        <DropdownMenu>
            <DropdownMenuTrigger class="inline-flex h-8 max-w-[180px] items-center justify-center rounded-lg border border-input bg-background px-3 text-sm font-medium shadow-sm transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring sm:max-w-[220px]">
                <span class="flex items-center gap-2 overflow-hidden">
                    <span class=move || {
                        let base = "h-1.5 w-1.5 shrink-0 rounded-full";
                        if has_model.get() {
                            format!("{base} bg-green-500")
                        } else {
                            format!("{base} bg-muted-foreground/40")
                        }
                    } />
                    <span class="truncate font-medium">{display_text}</span>
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="shrink-0 text-muted-foreground"><path d="m6 9 6 6 6-6"/></svg>
                </span>
            </DropdownMenuTrigger>
            <DropdownMenuContent align=crate::components::ui::dropdown_menu::DropdownAlign::End class="max-h-[70vh] w-[min(22rem,calc(100vw-2rem))] overflow-y-auto scroll-area">
                {content}
            </DropdownMenuContent>
        </DropdownMenu>
    }
}
