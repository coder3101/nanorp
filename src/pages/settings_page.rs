use leptos::prelude::*;
use leptos::callback::Callable;
use leptos::wasm_bindgen::JsCast;
use uuid::Uuid;
use crate::components::ui::classes::{BTN_OUTLINE, BTN_PRIMARY, INPUT};
use crate::components::ui::confirm::confirm;
use crate::components::ui::field::Field;
use crate::components::ui::modal::Modal;
use crate::components::ui::select::{Select, SelectOption};
use crate::components::ui::toast::{use_toast, ToastVariant};
use crate::models::provider::{ConnectionStatus, NewProvider, Provider, ProviderType, UpdateProvider};
use crate::models::settings::AppSettings;
use crate::server::provider::{
    create_provider, delete_provider, list_providers, test_provider_connection, update_provider,
};
use crate::server::settings::{get_settings, update_settings};
use crate::theme;

#[component]
pub fn SettingsPage() -> impl IntoView {
    let toast = use_toast();

    let active_tab = RwSignal::new("providers".to_string());

    // -- Data loading -------------------------------------------------------
    // Bumping this signal forces the providers list to refetch.
    let providers_version = RwSignal::new(0u32);
    let providers_resource = Resource::new(
        move || providers_version.get(),
        |_| async move { list_providers().await },
    );

    let settings_resource = Resource::new(|| (), |_| async move { get_settings().await });

    // Persisted theme (localStorage) drives the appearance picker.
    let theme_value = RwSignal::new(theme::DEFAULT_THEME.to_string());
    Effect::new(move |_| {
        theme_value.set(theme::get_theme());
    });

    // -- Provider dialog state ---------------------------------------------
    let provider_dialog_open = RwSignal::new(false);
    let editing_provider = RwSignal::new(Option::<Provider>::None);

    let prov_form_name = RwSignal::new(String::new());
    let prov_form_type = RwSignal::new("ollama".to_string());
    let prov_form_url = RwSignal::new(String::new());
    let prov_form_key = RwSignal::new(String::new());
    let prov_form_default = RwSignal::new(false);
    let prov_name_error = RwSignal::new(Option::<String>::None);
    let prov_url_error = RwSignal::new(Option::<String>::None);
    let saving = RwSignal::new(false);
    // Snapshot of the form as opened, for the unsaved-changes guard.
    let prov_snapshot = StoredValue::new((String::new(), String::new(), String::new(), String::new(), false));

    let is_editing_provider = Signal::derive(move || editing_provider.get().is_some());
    let provider_dialog_title = Signal::derive(move || {
        if is_editing_provider.get() { "Edit Provider" } else { "Add Provider" }.to_string()
    });

    let open_add_provider = Callback::new(move |_| {
        editing_provider.set(None);
        prov_form_name.set(String::new());
        prov_form_type.set("ollama".to_string());
        prov_form_url.set(String::new());
        prov_form_key.set(String::new());
        prov_form_default.set(false);
        prov_name_error.set(None);
        prov_url_error.set(None);
        prov_snapshot.set_value((String::new(), "ollama".to_string(), String::new(), String::new(), false));
        provider_dialog_open.set(true);
    });

    let open_edit_provider = Callback::new(move |p: Provider| {
        editing_provider.set(Some(p.clone()));
        let type_str = match p.provider_type {
            ProviderType::Ollama => "ollama".to_string(),
            ProviderType::OpenAiCompatible => "openai_compatible".to_string(),
        };
        let key = p.api_key.unwrap_or_default();
        prov_snapshot.set_value((p.name.clone(), type_str.clone(), p.api_url.clone(), key.clone(), p.is_default));
        prov_form_name.set(p.name);
        prov_form_type.set(type_str);
        prov_form_url.set(p.api_url);
        prov_form_key.set(key);
        prov_form_default.set(p.is_default);
        prov_name_error.set(None);
        prov_url_error.set(None);
        provider_dialog_open.set(true);
    });

    // Close the dialog, prompting first if there are unsaved changes.
    let close_provider_dialog = Callback::new(move |_: ()| {
        let current = (
            prov_form_name.get_untracked(),
            prov_form_type.get_untracked(),
            prov_form_url.get_untracked(),
            prov_form_key.get_untracked(),
            prov_form_default.get_untracked(),
        );
        if current == prov_snapshot.get_value() || confirm("Discard unsaved changes to this provider?") {
            provider_dialog_open.set(false);
            editing_provider.set(None);
        }
    });

    // -- Save (create or update) -------------------------------------------
    let toast_save = toast.clone();
    let save_provider = Callback::new(move |_| {
        let mut invalid = false;
        if prov_form_name.get().trim().is_empty() {
            prov_name_error.set(Some("Name is required".to_string()));
            invalid = true;
        }
        let url = prov_form_url.get().trim().to_string();
        if url.is_empty() {
            prov_url_error.set(Some("API URL is required".to_string()));
            invalid = true;
        } else if !url.starts_with("http://") && !url.starts_with("https://") {
            prov_url_error.set(Some("URL must start with http:// or https://".to_string()));
            invalid = true;
        }
        if invalid {
            return;
        }

        let provider_type = if prov_form_type.get() == "ollama" {
            ProviderType::Ollama
        } else {
            ProviderType::OpenAiCompatible
        };
        let key = Some(prov_form_key.get()).filter(|k| !k.is_empty());
        let editing = editing_provider.get();
        let toast_inner = toast_save.clone();
        saving.set(true);

        leptos::task::spawn_local(async move {
            let result = if let Some(existing) = editing {
                update_provider(UpdateProvider {
                    id: existing.id,
                    name: Some(prov_form_name.get_untracked()),
                    provider_type: Some(provider_type),
                    api_url: Some(prov_form_url.get_untracked()),
                    api_key: Some(key),
                    is_default: Some(prov_form_default.get_untracked()),
                })
                .await
                .map(|_| "Provider updated")
            } else {
                create_provider(NewProvider {
                    name: prov_form_name.get_untracked(),
                    provider_type,
                    api_url: prov_form_url.get_untracked(),
                    api_key: key,
                    is_default: prov_form_default.get_untracked(),
                })
                .await
                .map(|_| "Provider added")
            };

            saving.set(false);
            match result {
                Ok(msg) => {
                    toast_inner.success(msg);
                    provider_dialog_open.set(false);
                    editing_provider.set(None);
                    providers_version.update(|v| *v += 1);
                }
                Err(e) => toast_inner.error(format!("Save failed: {e}")),
            }
        });
    });

    // -- Delete -------------------------------------------------------------
    let toast_delete = toast.clone();
    let do_delete = Callback::new(move |id: Uuid| {
        let toast_inner = toast_delete.clone();
        leptos::task::spawn_local(async move {
            match delete_provider(id).await {
                Ok(()) => {
                    toast_inner.success("Provider removed");
                    providers_version.update(|v| *v += 1);
                }
                Err(e) => toast_inner.error(format!("Delete failed: {e}")),
            }
        });
    });

    // -- Test connection ----------------------------------------------------
    // Tracks per-provider connection status for badges.
    let statuses = RwSignal::new(std::collections::HashMap::<Uuid, ConnectionStatus>::new());
    let toast_test = toast.clone();
    let test_connection = Callback::new(move |id: Uuid| {
        statuses.update(|m| { m.insert(id, ConnectionStatus::Testing); });
        let toast_inner = toast_test.clone();
        leptos::task::spawn_local(async move {
            match test_provider_connection(id).await {
                Ok(status) => {
                    match &status {
                        ConnectionStatus::Connected => toast_inner.success("Connected"),
                        ConnectionStatus::Failed(reason) => {
                            toast_inner.custom("Connection failed", Some(reason.clone()), ToastVariant::Error, 5000);
                        }
                        ConnectionStatus::Testing => {}
                    }
                    statuses.update(|m| { m.insert(id, status); });
                }
                Err(e) => {
                    statuses.update(|m| { m.insert(id, ConnectionStatus::Failed(e.to_string())); });
                    toast_inner.error(format!("Test failed: {e}"));
                }
            }
        });
    });

    // -- General settings save ---------------------------------------------
    let settings_draft = RwSignal::new(AppSettings::default());
    // Last-loaded (or last-saved) settings, used to detect unsaved edits.
    let settings_saved = StoredValue::new(AppSettings::default());
    // Keep the draft in sync once settings load.
    Effect::new(move |_| {
        if let Some(Ok(s)) = settings_resource.get() {
            settings_saved.set_value(s.clone());
            settings_draft.set(s);
        }
    });

    let toast_settings = toast.clone();
    let general_saving = RwSignal::new(false);
    let save_general = Callback::new(move |_| {
        let draft = settings_draft.get();
        let toast_inner = toast_settings.clone();
        general_saving.set(true);
        leptos::task::spawn_local(async move {
            let res = update_settings(draft.clone()).await;
            general_saving.set(false);
            match res {
                Ok(()) => {
                    settings_saved.set_value(draft);
                    toast_inner.success("Settings saved");
                }
                Err(e) => toast_inner.error(format!("Save failed: {e}")),
            }
        });
    });

    // Switch tabs, warning if the General/Chat draft has unsaved edits.
    let switch_tab = Callback::new(move |id: &'static str| {
        let leaving_editable = matches!(active_tab.get_untracked().as_str(), "general" | "chat");
        let dirty = leaving_editable && settings_draft.get_untracked() != settings_saved.get_value();
        if dirty {
            if !confirm("You have unsaved settings changes. Discard them?") {
                return;
            }
            settings_draft.set(settings_saved.get_value());
        }
        active_tab.set(id.to_string());
    });

    let set_theme = Callback::new(move |t: String| {
        theme_value.set(t.clone());
        theme::set_theme(&t);
    });

    view! {
        <div class="flex flex-col h-full overflow-y-auto scroll-area">
            <div class="mx-auto w-full max-w-4xl px-4 py-8 sm:px-6 lg:px-8">
                <div class="mb-8">
                    <h1 class="text-3xl font-bold tracking-tight">"Settings"</h1>
                    <p class="mt-1 text-sm text-muted-foreground">
                        "Manage providers, defaults, and how NanoRP looks."
                    </p>
                </div>

                <div class="flex flex-col gap-8 lg:flex-row">
                    <nav class="flex gap-1 overflow-x-auto lg:w-52 lg:shrink-0 lg:flex-col">
                        <SettingsNavItem
                            id="providers" active=active_tab on_select=switch_tab label="Providers"
                            icon=view! {
                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="8" x="2" y="2" rx="2"/><rect width="20" height="8" x="2" y="14" rx="2"/><path d="M6 6h.01M6 18h.01"/></svg>
                            }.into_any()
                        />
                        <SettingsNavItem
                            id="general" active=active_tab on_select=switch_tab label="General"
                            icon=view! {
                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 7h-9M14 17H5"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/></svg>
                            }.into_any()
                        />
                        <SettingsNavItem
                            id="chat" active=active_tab on_select=switch_tab label="Chat"
                            icon=view! {
                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7.9 20A9 9 0 1 0 4 16.1L2 22Z"/></svg>
                            }.into_any()
                        />
                        <SettingsNavItem
                            id="appearance" active=active_tab on_select=switch_tab label="Appearance"
                            icon=view! {
                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="13.5" cy="6.5" r=".5" fill="currentColor"/><circle cx="17.5" cy="10.5" r=".5" fill="currentColor"/><circle cx="8.5" cy="7.5" r=".5" fill="currentColor"/><circle cx="6.5" cy="12.5" r=".5" fill="currentColor"/><path d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.926 0 1.648-.746 1.648-1.688 0-.437-.18-.835-.437-1.125-.29-.289-.438-.652-.438-1.125a1.64 1.64 0 0 1 1.668-1.668h1.996c3.051 0 5.555-2.503 5.555-5.554C21.965 6.012 17.461 2 12 2z"/></svg>
                            }.into_any()
                        />
                    </nav>

                    <div class="min-w-0 flex-1">
                        // ---- Providers ----
                        <Show when=move || active_tab.get() == "providers">
                            <SettingsSection
                                title="LLM Providers"
                                description="Connect to Ollama or any OpenAI-compatible endpoint. You can switch between them at any time."
                            >
                                <div class="space-y-3">
                                    <Transition fallback=|| view! { <ProvidersSkeleton /> }>
                                        {move || Suspend::new(async move {
                                            match providers_resource.await {
                                                Ok(list) if list.is_empty() => view! { <ProvidersEmpty /> }.into_any(),
                                                Ok(list) => view! {
                                                    <div class="space-y-3">
                                                        {list.into_iter().map(|p| view! {
                                                            <ProviderCard
                                                                provider=p
                                                                statuses=statuses
                                                                on_edit=open_edit_provider
                                                                on_delete=do_delete
                                                                on_test=test_connection
                                                            />
                                                        }).collect::<Vec<_>>()}
                                                    </div>
                                                }.into_any(),
                                                Err(e) => view! {
                                                    <div class="rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">
                                                        "Failed to load providers: " {e.to_string()}
                                                    </div>
                                                }.into_any(),
                                            }
                                        })}
                                    </Transition>

                                    <button
                                        class="mt-1 inline-flex w-full items-center justify-center gap-2 rounded-xl border border-dashed border-border bg-background px-4 py-3 text-sm font-medium text-muted-foreground transition-colors hover:border-primary/40 hover:bg-accent/50 hover:text-foreground"
                                        on:click=move |_| open_add_provider.run(())
                                    >
                                        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5v14"/></svg>
                                        "Add provider"
                                    </button>
                                </div>
                            </SettingsSection>
                        </Show>

                        // ---- General ----
                        <Show when=move || active_tab.get() == "general">
                            <SettingsSection
                                title="General"
                                description="Defaults applied to new conversations."
                            >
                                <Transition fallback=|| view! {
                                    <div class="space-y-6">
                                        <div class="h-16 animate-pulse rounded-lg bg-muted"></div>
                                        <div class="h-40 animate-pulse rounded-lg bg-muted"></div>
                                    </div>
                                }>
                                    {move || Suspend::new(async move {
                                        let _ = settings_resource.await;
                                        view! {
                                            <div class="space-y-6">
                                                <Field
                                                    label="Your name"
                                                    for_id="set-user-name"
                                                    hint="Used to replace the {{user}} placeholder in prompts."
                                                >
                                                    <input
                                                        id="set-user-name"
                                                        class=INPUT
                                                        placeholder="User"
                                                        prop:value=move || settings_draft.get().user_name
                                                        on:input=move |ev| {
                                                            let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                                                            settings_draft.update(|s| s.user_name = input.value());
                                                        }
                                                    />
                                                </Field>

                                                <Field
                                                    label="Default system prompt"
                                                    for_id="set-system-prompt"
                                                    hint="Used when a character doesn't define its own. Supports {{char}} and {{user}}."
                                                >
                                                    <textarea
                                                        id="set-system-prompt"
                                                        class=format!("{} min-h-[160px] resize-y leading-relaxed", INPUT)
                                                        prop:value=move || settings_draft.get().default_system_prompt
                                                        on:input=move |ev| {
                                                            let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlTextAreaElement>();
                                                            settings_draft.update(|s| s.default_system_prompt = input.value());
                                                        }
                                                    />
                                                </Field>

                                                <div class="flex justify-end">
                                                    <button
                                                        class=BTN_PRIMARY
                                                        disabled=move || general_saving.get()
                                                        on:click=move |_| save_general.run(())
                                                    >
                                                        {move || if general_saving.get() { "Saving..." } else { "Save changes" }}
                                                    </button>
                                                </div>
                                            </div>
                                        }
                                    })}
                                </Transition>
                            </SettingsSection>
                        </Show>

                        // ---- Chat / generation ----
                        <Show when=move || active_tab.get() == "chat">
                            <SettingsSection
                                title="Chat"
                                description="Control how responses are generated and displayed."
                            >
                                <Transition fallback=|| view! {
                                    <div class="space-y-6">
                                        <div class="h-16 animate-pulse rounded-lg bg-muted"></div>
                                        <div class="h-24 animate-pulse rounded-lg bg-muted"></div>
                                    </div>
                                }>
                                    {move || Suspend::new(async move {
                                        let _ = settings_resource.await;
                                        view! {
                                            <div class="space-y-6">
                                                // Render thinking toggle
                                                <label class="flex cursor-pointer items-center justify-between gap-3 rounded-lg border border-border p-4 hover:bg-accent/40">
                                                    <span class="text-sm">
                                                        <span class="font-medium">"Show reasoning"</span>
                                                        <span class="block text-xs text-muted-foreground">
                                                            "Display the model's \"thinking\" in a collapsible block (for reasoning models)."
                                                        </span>
                                                    </span>
                                                    <input
                                                        type="checkbox"
                                                        class="h-5 w-5 shrink-0 rounded border-border accent-primary"
                                                        prop:checked=move || settings_draft.get().render_thinking
                                                        on:change=move |ev| {
                                                            let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                                                            let checked = input.checked();
                                                            settings_draft.update(|s| s.render_thinking = checked);
                                                        }
                                                    />
                                                </label>

                                                <SliderField
                                                    label="Temperature"
                                                    hint="Higher values make output more random, lower more focused."
                                                    min=0.0 max=2.0 step=0.05
                                                    value=Signal::derive(move || settings_draft.get().temperature)
                                                    on_input=Callback::new(move |v: f32| settings_draft.update(|s| s.temperature = v))
                                                />

                                                <SliderField
                                                    label="Top-p (nucleus sampling)"
                                                    hint="Limits sampling to the most likely tokens. 1.0 disables it."
                                                    min=0.0 max=1.0 step=0.01
                                                    value=Signal::derive(move || settings_draft.get().top_p)
                                                    on_input=Callback::new(move |v: f32| settings_draft.update(|s| s.top_p = v))
                                                />

                                                <Field
                                                    label="Max tokens"
                                                    for_id="set-max-tokens"
                                                    hint="Maximum length of a reply. Leave empty for the provider default."
                                                >
                                                    <input
                                                        id="set-max-tokens"
                                                        class=INPUT
                                                        r#type="number"
                                                        min="1"
                                                        placeholder="Unlimited"
                                                        prop:value=move || settings_draft.get().max_tokens.map(|v| v.to_string()).unwrap_or_default()
                                                        on:input=move |ev| {
                                                            let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                                                            let val = input.value();
                                                            let parsed = val.trim().parse::<u32>().ok().filter(|v| *v > 0);
                                                            settings_draft.update(|s| s.max_tokens = parsed);
                                                        }
                                                    />
                                                </Field>

                                                <div class="flex justify-end">
                                                    <button
                                                        class=BTN_PRIMARY
                                                        disabled=move || general_saving.get()
                                                        on:click=move |_| save_general.run(())
                                                    >
                                                        {move || if general_saving.get() { "Saving..." } else { "Save changes" }}
                                                    </button>
                                                </div>
                                            </div>
                                        }
                                    })}
                                </Transition>
                            </SettingsSection>
                        </Show>

                        // ---- Appearance ----
                        <Show when=move || active_tab.get() == "appearance">
                            <SettingsSection
                                title="Appearance"
                                description="Customize the look and feel. Your choice is saved on this device."
                            >
                                <div>
                                    <p class="mb-3 text-sm font-medium">"Theme"</p>
                                    <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
                                        <ThemeOption value="light" label="Light" current=theme_value on_select=set_theme />
                                        <ThemeOption value="dark" label="Dark" current=theme_value on_select=set_theme />
                                        <ThemeOption value="system" label="System" current=theme_value on_select=set_theme />
                                    </div>
                                </div>
                            </SettingsSection>
                        </Show>
                    </div>
                </div>
            </div>

            // ---- Provider add/edit dialog ----
            <Modal
                open=provider_dialog_open
                label=provider_dialog_title
                class="max-w-lg p-6"
                on_close=close_provider_dialog
            >
                <h2 class="text-lg font-semibold">{provider_dialog_title}</h2>
                <p class="mt-1 text-sm text-muted-foreground">
                    "Configure how NanoRP reaches your model server."
                </p>

                <div class="mt-5 space-y-4">
                    <Field
                        label="Name"
                        for_id="prov-name"
                        error=Signal::derive(move || prov_name_error.get().unwrap_or_default())
                    >
                        <input
                            id="prov-name"
                            class=INPUT
                            placeholder="e.g. Local Ollama"
                            prop:value=move || prov_form_name.get()
                            on:input=move |ev| {
                                let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                                prov_form_name.set(input.value());
                                if !prov_form_name.get_untracked().trim().is_empty() {
                                    prov_name_error.set(None);
                                }
                            }
                        />
                    </Field>

                    <Field label="Type" for_id="prov-type">
                        <Select
                            id="prov-type"
                            value=prov_form_type
                            options=vec![
                                SelectOption::new("ollama", "Ollama"),
                                SelectOption::new("openai_compatible", "OpenAI Compatible"),
                            ]
                        />
                    </Field>

                    <Field
                        label="API URL"
                        for_id="prov-url"
                        error=Signal::derive(move || prov_url_error.get().unwrap_or_default())
                    >
                        <input
                            id="prov-url"
                            class=INPUT
                            placeholder="http://localhost:11434"
                            prop:value=move || prov_form_url.get()
                            on:input=move |ev| {
                                let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                                prov_form_url.set(input.value());
                                prov_url_error.set(None);
                            }
                        />
                    </Field>

                    <Show when=move || prov_form_type.get() == "openai_compatible">
                        <Field label="API Key" for_id="prov-key" hint="Stored locally. Sent only to the endpoint above.">
                            <input
                                type="password"
                                id="prov-key"
                                class=INPUT
                                placeholder="sk-..."
                                prop:value=move || prov_form_key.get()
                                on:input=move |ev| {
                                    let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                                    prov_form_key.set(input.value());
                                }
                            />
                        </Field>
                    </Show>

                    <label class="flex cursor-pointer items-center gap-3 rounded-lg border border-border p-3 hover:bg-accent/50">
                        <input
                            type="checkbox"
                            checked=move || prov_form_default.get()
                            on:change=move |_| prov_form_default.update(|v| *v = !*v)
                            class="h-4 w-4 rounded border-border accent-primary"
                        />
                        <span class="text-sm">
                            <span class="font-medium">"Set as default provider"</span>
                            <span class="block text-xs text-muted-foreground">
                                "Used automatically for new chats."
                            </span>
                        </span>
                    </label>
                </div>

                <div class="mt-6 flex justify-end gap-2">
                    <button class=BTN_OUTLINE on:click=move |_| close_provider_dialog.run(())>
                        "Cancel"
                    </button>
                    <button
                        class=BTN_PRIMARY
                        disabled=move || saving.get()
                        on:click=move |_| save_provider.run(())
                    >
                        {move || if saving.get() { "Saving..." } else { "Save provider" }}
                    </button>
                </div>
            </Modal>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

#[component]
fn SettingsNavItem(
    id: &'static str,
    active: RwSignal<String>,
    on_select: Callback<&'static str>,
    label: &'static str,
    icon: AnyView,
) -> impl IntoView {
    let is_active = Signal::derive(move || active.get() == id);
    view! {
        <button
            class=move || {
                let base = "flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm font-medium \
                            transition-colors whitespace-nowrap w-full text-left";
                if is_active.get() {
                    format!("{} bg-accent text-accent-foreground", base)
                } else {
                    format!("{} text-muted-foreground hover:bg-accent/50 hover:text-foreground", base)
                }
            }
            aria-current=move || if is_active.get() { "page" } else { "false" }
            on:click=move |_| on_select.run(id)
        >
            <span class="shrink-0">{icon}</span>
            {label}
        </button>
    }
}

#[component]
fn SettingsSection(
    title: &'static str,
    description: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="animate-fade-in">
            <div class="mb-5">
                <h2 class="text-lg font-semibold">{title}</h2>
                <p class="mt-0.5 text-sm text-muted-foreground">{description}</p>
            </div>
            {children()}
        </div>
    }
}

#[component]
fn SliderField(
    label: &'static str,
    #[prop(optional)] hint: &'static str,
    min: f32,
    max: f32,
    step: f32,
    value: Signal<f32>,
    on_input: Callback<f32>,
) -> impl IntoView {
    view! {
        <div class="space-y-1.5">
            <div class="flex items-center justify-between">
                <label class="text-sm font-medium">{label}</label>
                <span class="rounded bg-muted px-1.5 py-0.5 font-mono text-xs text-muted-foreground">
                    {move || format!("{:.2}", value.get())}
                </span>
            </div>
            {(!hint.is_empty()).then(|| view! {
                <p class="text-xs text-muted-foreground">{hint}</p>
            })}
            <input
                type="range"
                min=min.to_string()
                max=max.to_string()
                step=step.to_string()
                aria-label=label
                prop:value=move || value.get().to_string()
                class="h-2 w-full cursor-pointer appearance-none rounded-full bg-muted accent-primary"
                on:input=move |ev| {
                    let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                    if let Ok(v) = input.value().parse::<f32>() {
                        on_input.run(v);
                    }
                }
            />
        </div>
    }
}

#[component]
fn ThemeOption(
    value: &'static str,
    label: &'static str,
    current: RwSignal<String>,
    on_select: Callback<String>,
) -> impl IntoView {
    let is_active = Signal::derive(move || current.get() == value);

    let preview = match value {
        "light" => view! {
            <div class="flex h-full w-full flex-col gap-1 rounded-md bg-white p-2">
                <div class="h-1.5 w-8 rounded-full bg-zinc-800"></div>
                <div class="h-1.5 w-12 rounded-full bg-zinc-300"></div>
                <div class="mt-auto h-3 w-full rounded bg-zinc-100"></div>
            </div>
        }.into_any(),
        "dark" => view! {
            <div class="flex h-full w-full flex-col gap-1 rounded-md bg-zinc-900 p-2">
                <div class="h-1.5 w-8 rounded-full bg-zinc-100"></div>
                <div class="h-1.5 w-12 rounded-full bg-zinc-600"></div>
                <div class="mt-auto h-3 w-full rounded bg-zinc-800"></div>
            </div>
        }.into_any(),
        _ => view! {
            <div class="flex h-full w-full overflow-hidden rounded-md">
                <div class="flex w-1/2 flex-col gap-1 bg-white p-2">
                    <div class="h-1.5 w-6 rounded-full bg-zinc-800"></div>
                    <div class="mt-auto h-3 w-full rounded bg-zinc-100"></div>
                </div>
                <div class="flex w-1/2 flex-col gap-1 bg-zinc-900 p-2">
                    <div class="h-1.5 w-6 rounded-full bg-zinc-100"></div>
                    <div class="mt-auto h-3 w-full rounded bg-zinc-800"></div>
                </div>
            </div>
        }.into_any(),
    };

    view! {
        <button
            type="button"
            class=move || {
                let base = "group relative flex flex-col gap-3 rounded-xl border-2 p-3 text-left transition-all";
                if is_active.get() {
                    format!("{} border-primary ring-2 ring-primary/20", base)
                } else {
                    format!("{} border-border hover:border-muted-foreground/40", base)
                }
            }
            aria-pressed=move || is_active.get().to_string()
            on:click=move |_| on_select.run(value.to_string())
        >
            <div class="h-20 w-full overflow-hidden rounded-md border border-border/60 shadow-sm">
                {preview}
            </div>
            <div class="flex items-center justify-between">
                <span class="text-sm font-medium">{label}</span>
                <span
                    class=move || {
                        let base = "flex h-4 w-4 items-center justify-center rounded-full border transition-colors";
                        if is_active.get() {
                            format!("{} border-primary bg-primary text-primary-foreground", base)
                        } else {
                            format!("{} border-muted-foreground/40", base)
                        }
                    }
                >
                    <Show when=move || is_active.get()>
                        <svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
                    </Show>
                </span>
            </div>
        </button>
    }
}

#[component]
fn ProviderCard(
    provider: Provider,
    statuses: RwSignal<std::collections::HashMap<Uuid, ConnectionStatus>>,
    on_edit: Callback<Provider>,
    on_delete: Callback<Uuid>,
    on_test: Callback<Uuid>,
) -> impl IntoView {
    let p_id = provider.id;
    let prov_type_str = match provider.provider_type {
        ProviderType::Ollama => "Ollama",
        ProviderType::OpenAiCompatible => "OpenAI Compatible",
    };
    let p_name = provider.name.clone();
    let p_url = provider.api_url.clone();
    let p_api_key = provider.api_key.clone();
    let p_is_default = provider.is_default;
    let p_for_edit = provider.clone();

    // Status dot color driven by the last test result.
    let dot = move || {
        match statuses.get().get(&p_id) {
            Some(ConnectionStatus::Connected) => "bg-green-500",
            Some(ConnectionStatus::Failed(_)) => "bg-red-500",
            Some(ConnectionStatus::Testing) => "bg-yellow-500",
            None => "bg-muted-foreground/40",
        }
    };
    let testing = move || matches!(statuses.get().get(&p_id), Some(ConnectionStatus::Testing));

    // Two-step inline delete confirmation, auto-reverting after a few seconds.
    let confirming = RwSignal::new(false);
    let arm_confirm = move || {
        confirming.set(true);
        leptos::leptos_dom::helpers::set_timeout(
            move || confirming.set(false),
            std::time::Duration::from_secs(4),
        );
    };

    view! {
        <div class="rounded-xl border border-border bg-card text-card-foreground p-4 shadow-sm transition-shadow hover:shadow-md">
            <div class="flex items-start justify-between gap-3">
                <div class="flex min-w-0 items-center gap-3">
                    <span class=move || format!("relative flex h-2.5 w-2.5 shrink-0 rounded-full {}", dot()) />
                    <div class="min-w-0">
                        <div class="flex items-center gap-2">
                            <h3 class="truncate font-semibold">{p_name}</h3>
                            {p_is_default.then(|| view! {
                                <span class="inline-flex shrink-0 items-center rounded-full bg-primary/10 px-2 py-0.5 text-[11px] font-semibold text-primary">
                                    "Default"
                                </span>
                            })}
                        </div>
                        <p class="mt-0.5 truncate text-xs text-muted-foreground">
                            {prov_type_str} " · " {p_url}
                        </p>
                        {p_api_key.map(|k| view! {
                            <p class="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
                                "key " {mask_key(&k)}
                            </p>
                        })}
                    </div>
                </div>

                <div class="flex shrink-0 items-center gap-1">
                    <button
                        class="inline-flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                        title="Edit"
                        aria-label="Edit provider"
                        on:click=move |_| on_edit.run(p_for_edit.clone())
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.12 2.12 0 0 1 3 3L12 15l-4 1 1-4Z"/></svg>
                    </button>
                    <Show
                        when=move || confirming.get()
                        fallback=move || view! {
                            <button
                                class="inline-flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-destructive hover:text-destructive-foreground"
                                title="Delete"
                                aria-label="Delete provider"
                                on:click=move |_| arm_confirm()
                            >
                                <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
                            </button>
                        }
                    >
                        <button
                            class="inline-flex h-8 items-center gap-1 rounded-md bg-destructive px-2 text-xs font-medium text-destructive-foreground shadow-sm hover:bg-destructive/90"
                            aria-label="Confirm delete provider"
                            on:click=move |_| {
                                confirming.set(false);
                                on_delete.run(p_id);
                            }
                        >
                            <svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
                            "Confirm"
                        </button>
                        <button
                            class="inline-flex h-8 w-8 items-center justify-center rounded-md border border-border bg-background text-muted-foreground shadow-sm hover:bg-accent"
                            aria-label="Cancel delete"
                            on:click=move |_| confirming.set(false)
                        >
                            <svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
                        </button>
                    </Show>
                </div>
            </div>

            <div class="mt-3 border-t border-border/60 pt-3">
                <button
                    class="inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-60"
                    disabled=testing
                    on:click=move |_| on_test.run(p_id)
                >
                    <svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/></svg>
                    {move || if testing() { "Testing..." } else { "Test connection" }}
                </button>
            </div>
        </div>
    }
}

#[component]
fn ProvidersEmpty() -> impl IntoView {
    view! {
        <div class="flex flex-col items-center justify-center rounded-xl border border-dashed border-border py-12 text-center">
            <div class="mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-muted text-muted-foreground">
                <svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="8" x="2" y="2" rx="2"/><rect width="20" height="8" x="2" y="14" rx="2"/><path d="M6 6h.01M6 18h.01"/></svg>
            </div>
            <p class="text-sm font-medium">"No providers yet"</p>
            <p class="mt-1 max-w-xs text-xs text-muted-foreground">
                "Add a provider to start chatting with local or hosted models."
            </p>
        </div>
    }
}

#[component]
fn ProvidersSkeleton() -> impl IntoView {
    view! {
        <div class="space-y-3">
            {(0..2).map(|_| view! {
                <div class="rounded-xl border border-border p-4">
                    <div class="flex items-center gap-3">
                        <div class="h-2.5 w-2.5 rounded-full bg-muted"></div>
                        <div class="flex-1 space-y-2">
                            <div class="h-4 w-1/3 animate-pulse rounded bg-muted"></div>
                            <div class="h-3 w-2/3 animate-pulse rounded bg-muted"></div>
                        </div>
                    </div>
                </div>
            }).collect::<Vec<_>>()}
        </div>
    }
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "••••".to_string();
    }
    format!("{}••••", &key[..4])
}
