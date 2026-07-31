use crate::components::avatar::{gradient as avatar_gradient, initial};
use crate::components::chat::model_selector::ModelSelector;
use crate::components::ui::classes::{BTN_DESTRUCTIVE, BTN_OUTLINE, BTN_PRIMARY, INPUT};
use crate::components::ui::confirm::confirm;
use crate::components::ui::dropdown_menu::{
    DropdownAlign, DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator,
    DropdownMenuTrigger,
};
use crate::components::ui::field::Field;
use crate::components::ui::modal::Modal;
use crate::components::ui::toast::{use_toast, ToastVariant};
use crate::models::character::{Character, NewCharacter, UpdateCharacter};
use crate::server::character::{
    create_character, delete_character, generate_character, list_characters,
    remove_character_avatar, update_character, upload_character_avatar,
};
use leptos::callback::Callable;
use leptos::html;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;
use uuid::Uuid;

/// A pending avatar chosen in the dialog before it's uploaded.
#[derive(Clone, PartialEq)]
struct PendingAvatar {
    /// Raw base64 (no data-URL prefix) — what we send to the server.
    data: String,
    content_type: String,
    /// data-URL used purely for local preview.
    preview: String,
}

#[component]
pub fn CharactersPage() -> impl IntoView {
    let toast = use_toast();

    // Refetch trigger + data source.
    let version = RwSignal::new(0u32);
    let characters_res = Resource::new(
        move || version.get(),
        |_| async move { list_characters().await },
    );

    // Client-side name/role/personality filter.
    let query = RwSignal::new(String::new());

    let dialog_open = RwSignal::new(false);
    let editing_character = RwSignal::new(Option::<Character>::None);
    let delete_dialog_open = RwSignal::new(false);
    let deleting_character = RwSignal::new(Option::<Character>::None);
    let saving = RwSignal::new(false);

    let is_editing = Signal::derive(move || editing_character.get().is_some());
    let dialog_title = Signal::derive(move || {
        if is_editing.get() {
            "Edit Character"
        } else {
            "New Character"
        }
        .to_string()
    });
    let dialog_subtitle = Signal::derive(move || {
        if is_editing.get() {
            "Update this character's details.".to_string()
        } else {
            "Give your character a name, personality, and voice.".to_string()
        }
    });

    let form_name = RwSignal::new(String::new());
    let form_role = RwSignal::new(String::new());
    let form_personality = RwSignal::new(String::new());
    let form_system_prompt = RwSignal::new(String::new());
    let form_greeting = RwSignal::new(String::new());
    let name_error = RwSignal::new(Option::<String>::None);
    // Avatar dialog state:
    //  - pending_avatar: a newly-picked image not yet uploaded
    //  - existing_avatar: the currently-saved avatar_path (when editing)
    //  - avatar_cleared: user removed the existing avatar
    let pending_avatar = RwSignal::new(Option::<PendingAvatar>::None);
    let existing_avatar = RwSignal::new(Option::<String>::None);
    let avatar_cleared = RwSignal::new(false);
    // Snapshot of the form as opened, for the unsaved-changes guard.
    let form_snapshot = StoredValue::new((
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    ));

    // Generate-with-AI: a separate dialog. It is only surfaced once at least
    // one provider is configured (the AI can't run otherwise).
    let ai_open = RwSignal::new(false);
    let ai_description = RwSignal::new(String::new());
    let ai_provider = RwSignal::new(Option::<uuid::Uuid>::None);
    let ai_model = RwSignal::new(Option::<String>::None);
    let ai_generating = RwSignal::new(false);
    let ai_error = RwSignal::new(Option::<String>::None);
    let ai_success = RwSignal::new(Option::<String>::None);
    // Monotonically-increasing token for the in-flight generation request. A
    // request may only apply its result if its captured token still matches and
    // the dialog is still open. Bumping it on open and on every new generate
    // invalidates stale drafts, so closing the dialog or starting over safely
    // discards an in-flight result instead of overwriting whatever form the
    // user has navigated to.
    let ai_request = RwSignal::new(0u64);

    // Whether at least one LLM provider exists, which gates the "Generate with
    // AI" entry point in the create dialog.
    let providers_res = LocalResource::new(|| async move {
        crate::server::provider::list_providers()
            .await
            .is_ok_and(|p| !p.is_empty())
    });
    let has_providers = Signal::derive(move || providers_res.get().unwrap_or(false));

    let open_ai = Callback::new(move |_| {
        // Invalidate any in-flight request from a previous session.
        ai_request.update(|v| *v += 1);
        ai_error.set(None);
        ai_success.set(None);
        ai_generating.set(false);
        ai_open.set(true);
    });

    let open_create = Callback::new(move |_| {
        editing_character.set(None);
        form_name.set(String::new());
        form_role.set(String::new());
        form_personality.set(String::new());
        form_system_prompt.set(String::new());
        form_greeting.set(String::new());
        name_error.set(None);
        pending_avatar.set(None);
        existing_avatar.set(None);
        avatar_cleared.set(false);
        form_snapshot.set_value(Default::default());
        ai_error.set(None);
        ai_success.set(None);
        ai_generating.set(false);
        dialog_open.set(true);
    });

    let open_edit = Callback::new(move |char: Character| {
        editing_character.set(Some(char.clone()));
        let name = char.name;
        let role = char.role.unwrap_or_default();
        let personality = char.personality.unwrap_or_default();
        let system_prompt = char.system_prompt.unwrap_or_default();
        let greeting = char.greeting.unwrap_or_default();
        form_snapshot.set_value((
            name.clone(),
            role.clone(),
            personality.clone(),
            system_prompt.clone(),
            greeting.clone(),
        ));
        form_name.set(name);
        form_role.set(role);
        form_personality.set(personality);
        form_system_prompt.set(system_prompt);
        form_greeting.set(greeting);
        name_error.set(None);
        pending_avatar.set(None);
        existing_avatar.set(char.avatar_path);
        avatar_cleared.set(false);
        dialog_open.set(true);
    });

    // Close the editor, prompting first if there are unsaved changes.
    let close_dialog = Callback::new(move |_: ()| {
        let current = (
            form_name.get_untracked(),
            form_role.get_untracked(),
            form_personality.get_untracked(),
            form_system_prompt.get_untracked(),
            form_greeting.get_untracked(),
        );
        let dirty = current != form_snapshot.get_value()
            || pending_avatar.get_untracked().is_some()
            || avatar_cleared.get_untracked();
        if !dirty || confirm("Discard unsaved changes to this character?") {
            dialog_open.set(false);
            editing_character.set(None);
        }
    });

    let confirm_delete = Callback::new(move |char: Character| {
        deleting_character.set(Some(char));
        delete_dialog_open.set(true);
    });

    let toast_delete = toast.clone();
    let do_delete = Callback::new(move |_| {
        let Some(char) = deleting_character.get() else {
            return;
        };
        let toast_inner = toast_delete.clone();
        leptos::task::spawn_local(async move {
            match delete_character(char.id).await {
                Ok(()) => {
                    toast_inner.success(format!("Deleted {}", char.name));
                    delete_dialog_open.set(false);
                    deleting_character.set(None);
                    version.update(|v| *v += 1);
                }
                Err(e) => toast_inner.error(format!("Delete failed: {e}")),
            }
        });
    });

    let toast_save = toast.clone();
    let save_character = Callback::new(move |_| {
        if form_name.get().trim().is_empty() {
            name_error.set(Some("Name is required".to_string()));
            return;
        }

        let editing = editing_character.get();
        let pending = pending_avatar.get();
        let cleared = avatar_cleared.get();
        let toast_inner = toast_save.clone();
        saving.set(true);

        leptos::task::spawn_local(async move {
            // 1. Persist the text fields (create or update).
            let saved: Result<Character, _> = if let Some(existing) = editing {
                update_character(UpdateCharacter {
                    id: existing.id,
                    name: Some(form_name.get_untracked()),
                    role: Some(form_role.get_untracked()),
                    personality: Some(form_personality.get_untracked()),
                    system_prompt: Some(form_system_prompt.get_untracked()),
                    greeting: Some(form_greeting.get_untracked()),
                })
                .await
            } else {
                create_character(NewCharacter {
                    name: form_name.get_untracked(),
                    role: Some(form_role.get_untracked()).filter(|s| !s.is_empty()),
                    personality: Some(form_personality.get_untracked()).filter(|s| !s.is_empty()),
                    system_prompt: Some(form_system_prompt.get_untracked())
                        .filter(|s| !s.is_empty()),
                    greeting: Some(form_greeting.get_untracked()).filter(|s| !s.is_empty()),
                })
                .await
            };

            let character = match saved {
                Ok(c) => c,
                Err(e) => {
                    saving.set(false);
                    toast_inner.error(format!("Save failed: {e}"));
                    return;
                }
            };

            // 2. Apply avatar changes.
            if let Some(p) = pending {
                if let Err(e) = upload_character_avatar(character.id, p.data, p.content_type).await
                {
                    toast_inner.custom(
                        "Avatar upload failed",
                        Some(e.to_string()),
                        ToastVariant::Warning,
                        4000,
                    );
                }
            } else if cleared {
                if let Err(e) = remove_character_avatar(character.id).await {
                    toast_inner.custom(
                        "Couldn't remove avatar",
                        Some(e.to_string()),
                        ToastVariant::Warning,
                        4000,
                    );
                }
            }

            saving.set(false);
            toast_inner.success(if is_editing.get_untracked() {
                "Character updated"
            } else {
                "Character created"
            });
            dialog_open.set(false);
            editing_character.set(None);
            version.update(|v| *v += 1);
        });
    });

    // Generate-with-AI: ask the chosen LLM to draft a character from the
    // described situation, then drop the result into the reviewable form
    // fields (the user saves or retries from there).
    let toast_generate = toast.clone();
    let generate_with_ai = Callback::new(move |_| {
        let description = ai_description.get().trim().to_string();
        if description.is_empty() {
            ai_error.set(Some("Describe the character you want first.".to_string()));
            return;
        }
        let (Some(provider_id), Some(model)) = (ai_provider.get(), ai_model.get()) else {
            ai_error.set(Some("Choose a provider and model first.".to_string()));
            return;
        };

        ai_error.set(None);
        ai_success.set(None);
        ai_generating.set(true);

        // Invalidate any prior in-flight request, then capture this one's token.
        ai_request.update(|v| *v += 1);
        let token = ai_request.get_untracked();

        let toast_generate = toast_generate.clone();
        leptos::task::spawn_local(async move {
            // Discard the result if a newer request superseded this one or the
            // dialog was closed while it was in flight. This keeps a stale
            // generation from overwriting form fields the user has since
            // changed or a character they've moved on to.
            let stale = move || ai_request.get_untracked() != token || !ai_open.get_untracked();

            match generate_character(provider_id, model, description).await {
                Ok(draft) => {
                    if stale() {
                        return;
                    }
                    form_name.set(draft.name.clone());
                    form_role.set(draft.role.unwrap_or_default());
                    form_personality.set(draft.personality.unwrap_or_default());
                    form_system_prompt.set(draft.system_prompt.unwrap_or_default());
                    form_greeting.set(draft.greeting.unwrap_or_default());
                    name_error.set(None);
                    // Reset the unsaved-changes snapshot so the generated draft
                    // is treated as the clean baseline, not "changed" form data.
                    form_snapshot.set_value((
                        form_name.get_untracked(),
                        form_role.get_untracked(),
                        form_personality.get_untracked(),
                        form_system_prompt.get_untracked(),
                        form_greeting.get_untracked(),
                    ));
                    ai_success.set(Some(format!(
                        "Draft ready: \"{}\". Review the fields below, then save or retry.",
                        draft.name
                    )));
                    toast_generate.success(format!("Generated \"{}\"", draft.name));
                    ai_generating.set(false);
                }
                Err(e) => {
                    if stale() {
                        return;
                    }
                    ai_error.set(Some(e.to_string()));
                    ai_generating.set(false);
                }
            }
        });
    });

    let toast_chat = toast.clone();
    let start_chat = Callback::new(move |character_id: Uuid| {
        let toast_inner = toast_chat.clone();
        leptos::task::spawn_local(async move {
            match crate::server::chat::create_chat_session(character_id).await {
                Ok(session) => {
                    let navigate = leptos_router::hooks::use_navigate();
                    navigate(format!("/chat/{}", session.id).as_str(), Default::default());
                }
                Err(e) => toast_inner.error(format!("Couldn't start chat: {e}")),
            }
        });
    });

    let toast_dup = toast.clone();
    let duplicate_character = Callback::new(move |char: Character| {
        let toast_inner = toast_dup.clone();
        leptos::task::spawn_local(async move {
            let copy = NewCharacter {
                name: format!("{} (copy)", char.name),
                role: char.role.clone(),
                personality: char.personality.clone(),
                system_prompt: char.system_prompt.clone(),
                greeting: char.greeting.clone(),
            };
            match create_character(copy).await {
                Ok(c) => {
                    toast_inner.success(format!("Duplicated as {}", c.name));
                    version.update(|v| *v += 1);
                }
                Err(e) => toast_inner.error(format!("Duplicate failed: {e}")),
            }
        });
    });

    // Download the character (without avatar) as a JSON file.
    let export_character = Callback::new(move |_char: Character| {
        #[cfg(feature = "hydrate")]
        {
            let data = NewCharacter {
                name: _char.name.clone(),
                role: _char.role.clone(),
                personality: _char.personality.clone(),
                system_prompt: _char.system_prompt.clone(),
                greeting: _char.greeting.clone(),
            };
            if let Ok(json) = serde_json::to_string_pretty(&data) {
                let filename = format!("{}.json", _char.name.trim().replace('/', "-"));
                download_json(&filename, &json);
            }
        }
    });

    // Import a character from a previously exported JSON file.
    let import_input = NodeRef::<html::Input>::new();
    let trigger_import = Callback::new(move |_: ()| {
        if let Some(input) = import_input.get() {
            input.click();
        }
    });
    let toast_import = toast.clone();
    let handle_import = move |_ev: leptos::web_sys::Event| {
        #[cfg(feature = "hydrate")]
        {
            let input = _ev
                .target()
                .unwrap()
                .unchecked_into::<web_sys::HtmlInputElement>();
            if let Some(file) = input.files().and_then(|f| f.get(0)) {
                import_character_file(file, toast_import.clone(), version);
            }
            input.set_value("");
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = &toast_import;
    };

    // Handle avatar file selection in the dialog.
    let toast_avatar = toast.clone();
    let pick_avatar = move |_ev: leptos::web_sys::Event| {
        #[cfg(feature = "hydrate")]
        {
            let input = _ev
                .target()
                .unwrap()
                .unchecked_into::<web_sys::HtmlInputElement>();
            if let Some(files) = input.files() {
                if let Some(file) = files.get(0) {
                    read_avatar_file(file, pending_avatar, avatar_cleared, &toast_avatar);
                }
            }
            input.set_value("");
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = &toast_avatar;
    };

    // Computed preview URL for the dialog avatar area.
    let dialog_avatar_url = Signal::derive(move || {
        if let Some(p) = pending_avatar.get() {
            Some(p.preview)
        } else if avatar_cleared.get() {
            None
        } else {
            existing_avatar.get().map(|rel| format!("/{rel}"))
        }
    });
    let has_dialog_avatar = Signal::derive(move || dialog_avatar_url.get().is_some());

    view! {
        <div class="h-full overflow-y-auto scroll-area">
            <div class="mx-auto w-full max-w-6xl px-4 py-8 sm:px-6 lg:px-8">
                // Page header
                <div class="mb-8 flex flex-wrap items-end justify-between gap-4">
                    <div>
                        <div class="flex items-center gap-2.5">
                            <h1 class="text-3xl font-bold tracking-tight">"Characters"</h1>
                            <Transition>
                                {move || Suspend::new(async move {
                                    characters_res.await.ok().filter(|l| !l.is_empty()).map(|l| view! {
                                        <span class="rounded-full bg-muted px-2.5 py-0.5 text-sm font-medium text-muted-foreground">
                                            {l.len()}
                                        </span>
                                    })
                                })}
                            </Transition>
                        </div>
                        <p class="mt-1 text-sm text-muted-foreground">
                            "Create and manage the personas you chat with."
                        </p>
                    </div>
                    <input
                        type="file"
                        accept="application/json,.json"
                        class="hidden"
                        node_ref=import_input
                        on:change=handle_import
                    />
                    <Transition>
                        {move || Suspend::new(async move {
                            characters_res.await.ok().filter(|l| !l.is_empty()).map(|_| view! {
                                <div class="flex flex-wrap items-center gap-2">
                                    <div class="relative">
                                        <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24"
                                             fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
                                             class="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground">
                                            <circle cx="11" cy="11" r="8"/>
                                            <path d="m21 21-4.3-4.3"/>
                                        </svg>
                                        <input
                                            type="search"
                                            class=format!("{} h-10 w-48 pl-8", INPUT)
                                            placeholder="Search..."
                                            aria-label="Search characters"
                                            prop:value=move || query.get()
                                            on:input=move |ev| {
                                                let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                                                query.set(input.value());
                                            }
                                        />
                                    </div>
                                    <button class=BTN_OUTLINE on:click=move |_| trigger_import.run(())>
                                        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m8 11 4 4 4-4"/><path d="M8 5H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-4"/></svg>
                                        "Import"
                                    </button>
                                    <button class=BTN_PRIMARY on:click=move |_| open_create.run(())>
                                        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5v14"/></svg>
                                        "New character"
                                    </button>
                                </div>
                            })
                        })}
                    </Transition>
                </div>

                <Transition fallback=|| view! { <CharactersSkeleton /> }>
                    {move || Suspend::new(async move {
                        match characters_res.await {
                            Ok(list) if list.is_empty() => view! {
                                <CharactersEmpty on_create=open_create on_import=trigger_import />
                            }.into_any(),
                            Ok(list) => {
                                let all = StoredValue::new(list);
                                view! {
                                    {move || {
                                        let q = query.get().trim().to_lowercase();
                                        let filtered: Vec<Character> = all
                                            .get_value()
                                            .into_iter()
                                            .filter(|c| {
                                                q.is_empty()
                                                    || c.name.to_lowercase().contains(&q)
                                                    || c.role.as_deref().is_some_and(|r| r.to_lowercase().contains(&q))
                                                    || c.personality.as_deref().is_some_and(|p| p.to_lowercase().contains(&q))
                                            })
                                            .collect();
                                        if filtered.is_empty() {
                                            view! {
                                                <p class="rounded-xl border border-dashed border-border py-12 text-center text-sm text-muted-foreground">
                                                    "No characters match your search."
                                                </p>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <div class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
                                                    {filtered.into_iter().map(|char| view! {
                                                        <CharacterCard
                                                            character=char
                                                            on_start=start_chat
                                                            on_edit=open_edit
                                                            on_delete=confirm_delete
                                                            on_duplicate=duplicate_character
                                                            on_export=export_character
                                                        />
                                                    }).collect::<Vec<_>>()}
                                                </div>
                                            }.into_any()
                                        }
                                    }}
                                }.into_any()
                            }
                            Err(e) => view! {
                                <div class="rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">
                                    "Failed to load characters: " {e.to_string()}
                                </div>
                            }.into_any(),
                        }
                    })}
                </Transition>
            </div>

            // ---- Create / edit dialog ----
            <Modal open=dialog_open label=dialog_title class="max-w-lg" on_close=close_dialog>
                // Avatar + title header
                <div class="flex items-center gap-4 border-b border-border p-6">
                    <div class="relative shrink-0">
                        <Show
                            when=move || has_dialog_avatar.get()
                            fallback=move || view! {
                                <div class=move || format!(
                                    "flex h-16 w-16 items-center justify-center rounded-full bg-gradient-to-br {} text-2xl font-semibold text-white shadow-sm",
                                    avatar_gradient(&form_name.get())
                                )>
                                    {move || initial(&form_name.get())}
                                </div>
                            }
                        >
                            <img
                                src=move || dialog_avatar_url.get().unwrap_or_default()
                                alt="Character avatar"
                                class="h-16 w-16 rounded-full object-cover shadow-sm"
                            />
                        </Show>
                    </div>

                    <div class="min-w-0 flex-1">
                        <h2 class="text-lg font-semibold">{dialog_title}</h2>
                        <p class="mt-0.5 text-sm text-muted-foreground">{dialog_subtitle}</p>
                        <div class="mt-2 flex items-center gap-2">
                            <label class="inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md border border-input bg-background px-2.5 text-xs font-medium shadow-sm transition-colors hover:bg-accent">
                                <svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m8 11 4 4 4-4"/><path d="M8 5H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-4"/></svg>
                                {move || if has_dialog_avatar.get() { "Change" } else { "Upload image" }}
                                <input type="file" accept="image/*" class="hidden" on:change=pick_avatar.clone() />
                            </label>
                            <Show when=move || has_dialog_avatar.get()>
                                <button
                                    type="button"
                                    class="inline-flex h-8 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium text-muted-foreground transition-colors hover:bg-destructive hover:text-destructive-foreground"
                                    on:click=move |_| {
                                        pending_avatar.set(None);
                                        avatar_cleared.set(true);
                                    }
                                >
                                    "Remove"
                                </button>
                            </Show>
                        </div>
                    </div>
                </div>

                <div class="space-y-4 p-6">
                    // Entry point to the Generate-with-AI dialog. Only offered
                    // when at least one provider is configured, and only when
                    // creating a character (not editing).
                    <Show when=move || !is_editing.get() && has_providers.get()>
                        <button
                            type="button"
                            class="inline-flex w-full items-center justify-center gap-2 rounded-lg border border-dashed border-input bg-background px-4 py-3 text-sm font-medium text-muted-foreground shadow-sm transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                            on:click=move |_| open_ai.run(())
                        >
                            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/><path d="M19 3v4"/><path d="M21 5h-4"/></svg>
                            "Generate with AI"
                        </button>
                    </Show>

                    <Field
                        label="Name"
                        for_id="char-name"
                        hint="How the character is displayed."
                        error=Signal::derive(move || name_error.get().unwrap_or_default())
                    >
                        <input
                            id="char-name"
                            class=INPUT
                            placeholder="e.g. Gandalf"
                            prop:value=move || form_name.get()
                            on:input=move |ev| {
                                let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                                form_name.set(input.value());
                                if !form_name.get_untracked().trim().is_empty() {
                                    name_error.set(None);
                                }
                            }
                        />
                    </Field>

                    <Field label="Role" for_id="char-role" hint="A short tagline shown under the name.">
                        <input
                            id="char-role"
                            class=INPUT
                            placeholder="e.g. A wise old wizard"
                            prop:value=move || form_role.get()
                            on:input=move |ev| {
                                let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlInputElement>();
                                form_role.set(input.value());
                            }
                        />
                    </Field>

                    <Field label="Personality" for_id="char-personality" hint="Traits, mannerisms, and background.">
                        <textarea
                            id="char-personality"
                            class=format!("{} min-h-[80px] resize-y leading-relaxed", INPUT)
                            placeholder="Curious, warm, a little mischievous..."
                            prop:value=move || form_personality.get()
                            on:input=move |ev| {
                                let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlTextAreaElement>();
                                form_personality.set(input.value());
                            }
                        />
                    </Field>

                    <Field label="System prompt" for_id="char-system-prompt" hint="Instructions for the AI. Supports {{char}} and {{user}}.">
                        <textarea
                            id="char-system-prompt"
                            class=format!("{} min-h-[120px] resize-y leading-relaxed", INPUT)
                            placeholder="You are {{char}}, speaking with {{user}}..."
                            prop:value=move || form_system_prompt.get()
                            on:input=move |ev| {
                                let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlTextAreaElement>();
                                form_system_prompt.set(input.value());
                            }
                        />
                    </Field>

                    <Field label="Greeting" for_id="char-greeting" hint="The first message sent when a chat begins.">
                        <textarea
                            id="char-greeting"
                            class=format!("{} min-h-[80px] resize-y leading-relaxed", INPUT)
                            placeholder="Ah, a visitor! Come in, come in..."
                            prop:value=move || form_greeting.get()
                            on:input=move |ev| {
                                let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlTextAreaElement>();
                                form_greeting.set(input.value());
                            }
                        />
                    </Field>
                </div>

                <div class="flex justify-end gap-2 border-t border-border p-6 pt-4">
                    <button class=BTN_OUTLINE on:click=move |_| close_dialog.run(())>
                        "Cancel"
                    </button>
                    <button class=BTN_PRIMARY disabled=move || saving.get() on:click=move |_| save_character.run(())>
                        {move || if saving.get() {
                            "Saving...".to_string()
                        } else if is_editing.get() {
                            "Save changes".to_string()
                        } else {
                            "Create character".to_string()
                        }}
                    </button>
                </div>
            </Modal>

            // ---- Delete confirmation dialog ----
            <Modal
                open=delete_dialog_open
                label=Signal::derive(|| "Delete character".to_string())
                class="max-w-md p-6"
            >
                <div class="mb-4 flex h-11 w-11 items-center justify-center rounded-full bg-destructive/10 text-destructive">
                    <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
                </div>
                <h2 class="text-lg font-semibold">"Delete character?"</h2>
                <p class="mt-1.5 text-sm text-muted-foreground">
                    {move || {
                        deleting_character.get().map(|c| {
                            format!("This permanently deletes \"{}\" and all of its chat sessions. This action cannot be undone.", c.name)
                        }).unwrap_or_default()
                    }}
                </p>
                <div class="mt-6 flex justify-end gap-2">
                    <button
                        class=BTN_OUTLINE
                        on:click=move |_| { delete_dialog_open.set(false); deleting_character.set(None); }
                    >
                        "Cancel"
                    </button>
                    <button class=BTN_DESTRUCTIVE on:click=move |_| do_delete.run(())>
                        "Delete"
                    </button>
                </div>
            </Modal>

            // ---- Generate-with-AI dialog ----
            // Its own popup so the create form stays focused on manual entry.
            // The description, model picker, and generation lifecycle all live
            // here; a successful draft is dropped into the create form.
            <Modal
                open=ai_open
                label=Signal::derive(|| "Generate with AI".to_string())
                class="w-full max-w-md p-0 sm:p-0"
            >
                <div class="flex items-start justify-between gap-4 border-b border-border p-5 sm:p-6">
                    <div class="flex items-start gap-3">
                        <span class="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-violet-500 to-fuchsia-600 text-white shadow-sm">
                            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/><path d="M19 3v4"/><path d="M21 5h-4"/></svg>
                        </span>
                        <div class="min-w-0">
                            <h2 class="text-base font-semibold">"Generate with AI"</h2>
                            <p class="mt-0.5 text-xs text-muted-foreground">
                                "Describe the character you want and let the model draft it. You can review and edit everything before saving."
                            </p>
                        </div>
                    </div>
                    <button
                        type="button"
                        class="-mr-1 -mt-1 inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                        aria-label="Close"
                        on:click=move |_| ai_open.set(false)
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
                    </button>
                </div>

                <div class="space-y-4 p-5 sm:p-6">
                    <div class="space-y-1.5">
                        <label class="text-sm font-medium" r#for="ai-description">"Describe your character"</label>
                        <textarea
                            id="ai-description"
                            rows="4"
                            placeholder="e.g. A gruff retired ship captain who runs a dockside tavern and takes on odd jobs for {{user}}..."
                            class=format!("{} min-h-[120px] resize-y leading-relaxed", INPUT)
                            prop:value=move || ai_description.get()
                            on:input=move |ev| {
                                let input = ev.target().unwrap().unchecked_into::<leptos::web_sys::HtmlTextAreaElement>();
                                ai_description.set(input.value());
                            }
                        ></textarea>
                    </div>

                    <div class="flex flex-wrap items-center justify-between gap-3">
                        <span class="text-sm font-medium">"Model"</span>
                        <ModelSelector selected_provider=ai_provider selected_model=ai_model />
                    </div>

                    <Show when=move || ai_error.get().is_some()>
                        <div class="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2.5 text-xs text-destructive">
                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="mt-0.5 shrink-0"><circle cx="12" cy="12" r="10"/><path d="M12 8v4"/><path d="M12 16h.01"/></svg>
                            <span>{move || ai_error.get().unwrap_or_default()}</span>
                        </div>
                    </Show>
                    <Show when=move || ai_success.get().is_some()>
                        <div class="flex items-start gap-2 rounded-md border border-emerald-500/30 bg-emerald-500/10 px-3 py-2.5 text-xs text-emerald-700 dark:text-emerald-400">
                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="mt-0.5 shrink-0"><path d="M20 6 9 17l-5-5"/></svg>
                            <span>{move || ai_success.get().unwrap_or_default()}</span>
                        </div>
                    </Show>
                </div>

                // Footer: Close always; the primary action switches from
                // "Generate" to "Use these details" once a draft is ready, with
                // "Regenerate" available alongside it. Column-stacked on mobile
                // so the primary action sits on top (thumb-friendly).
                <div class="flex flex-col-reverse gap-2 border-t border-border p-5 pt-4 sm:flex-row sm:items-center sm:justify-end sm:p-6 sm:pt-4">
                    <button
                        type="button"
                        class=BTN_OUTLINE
                        on:click=move |_| ai_open.set(false)
                    >
                        "Close"
                    </button>

                    <Show
                        when=move || ai_success.get().is_some()
                        fallback=move || view! {
                            <button
                                type="button"
                                class=BTN_PRIMARY
                                disabled=move || ai_generating.get()
                                on:click=move |_| generate_with_ai.run(())
                            >
                                {move || if ai_generating.get() {
                                    view! {
                                        <svg class="h-4 w-4 animate-spin" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-6.219-8.56"/></svg>
                                        "Generating..."
                                    }.into_any()
                                } else {
                                    view! {
                                        <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/><path d="M19 3v4"/><path d="M21 5h-4"/></svg>
                                        "Generate character"
                                    }.into_any()
                                }}
                            </button>
                        }
                    >
                        <button
                            type="button"
                            class=BTN_OUTLINE
                            on:click=move |_| generate_with_ai.run(())
                        >
                            <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/></svg>
                            "Regenerate"
                        </button>
                        <button
                            type="button"
                            class=BTN_PRIMARY
                            on:click=move |_| ai_open.set(false)
                        >
                            "Use these details"
                        </button>
                    </Show>
                </div>
            </Modal>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Client-side file helpers
// ---------------------------------------------------------------------------

#[cfg(feature = "hydrate")]
fn read_avatar_file(
    file: web_sys::File,
    pending: RwSignal<Option<PendingAvatar>>,
    cleared: RwSignal<bool>,
    toast: &crate::components::ui::toast::UseToast,
) {
    use leptos::wasm_bindgen::closure::Closure;
    use leptos::wasm_bindgen::JsCast;

    let content_type = file.type_();
    if !content_type.starts_with("image/") {
        toast.warning("Only image files can be used as an avatar");
        return;
    }
    if file.size() as u64 > 5 * 1024 * 1024 {
        toast.warning("Avatar image is too large (max 5 MB)");
        return;
    }

    let reader = web_sys::FileReader::new().unwrap();
    let reader_clone = reader.clone();
    let ct = content_type.clone();

    let onload = Closure::wrap(Box::new(move |_: web_sys::Event| {
        if let Ok(result) = reader_clone.result() {
            if let Some(data_url) = result.as_string() {
                let base64 = data_url.split(',').nth(1).unwrap_or("").to_string();
                pending.set(Some(PendingAvatar {
                    data: base64,
                    content_type: ct.clone(),
                    preview: data_url,
                }));
                cleared.set(false);
            }
        }
    }) as Box<dyn FnMut(_)>);

    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
    onload.forget();
    let _ = reader.read_as_data_url(&file);
}

/// Read an exported character JSON file and create the character.
#[cfg(feature = "hydrate")]
fn import_character_file(
    file: web_sys::File,
    toast: crate::components::ui::toast::UseToast,
    version: RwSignal<u32>,
) {
    use leptos::wasm_bindgen::closure::Closure;
    use leptos::wasm_bindgen::JsCast;

    let reader = web_sys::FileReader::new().unwrap();
    let reader_clone = reader.clone();

    let onload = Closure::wrap(Box::new(move |_: web_sys::Event| {
        let Ok(result) = reader_clone.result() else {
            return;
        };
        let Some(text) = result.as_string() else {
            return;
        };
        let toast = toast.clone();
        match serde_json::from_str::<NewCharacter>(&text) {
            Ok(nc) if !nc.name.trim().is_empty() => {
                leptos::task::spawn_local(async move {
                    match create_character(nc).await {
                        Ok(c) => {
                            toast.success(format!("Imported {}", c.name));
                            version.update(|v| *v += 1);
                        }
                        Err(e) => toast.error(format!("Import failed: {e}")),
                    }
                });
            }
            Ok(_) => toast.error("Import failed: the character has no name"),
            Err(_) => toast.error("Import failed: not a valid character file"),
        }
    }) as Box<dyn FnMut(_)>);

    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
    onload.forget();
    let _ = reader.read_as_text(&file);
}

/// Trigger a browser download of a JSON string via a temporary data-URL link.
#[cfg(feature = "hydrate")]
fn download_json(filename: &str, contents: &str) {
    use leptos::wasm_bindgen::JsCast;

    let Some(doc) = leptos::web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(anchor) = doc.create_element("a") else {
        return;
    };
    let encoded = String::from(js_sys::encode_uri_component(contents));
    let _ = anchor.set_attribute(
        "href",
        &format!("data:application/json;charset=utf-8,{encoded}"),
    );
    let _ = anchor.set_attribute("download", filename);
    if let Some(el) = anchor.dyn_ref::<leptos::web_sys::HtmlElement>() {
        el.click();
    }
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

#[component]
fn CharacterCard(
    character: Character,
    on_start: Callback<Uuid>,
    on_edit: Callback<Character>,
    on_delete: Callback<Character>,
    on_duplicate: Callback<Character>,
    on_export: Callback<Character>,
) -> impl IntoView {
    let id = character.id;
    let name = character.name.clone();
    let gradient = avatar_gradient(&character.name);
    let av_initial = initial(&character.name);
    let role = character.role.clone();
    let personality = character.personality.clone();
    let avatar_url = character.avatar_path.clone().map(|rel| format!("/{rel}"));
    let has_avatar = avatar_url.is_some();
    let for_edit = character.clone();
    let for_delete = character.clone();
    let for_duplicate = character.clone();
    let for_export = character.clone();

    view! {
        // No `overflow-hidden` here: the actions dropdown opens downward from the
        // bottom edge of this card and would be clipped away. `focus-within:z-20`
        // lifts the card (and the open menu) above its neighbours, which matters
        // because the hover transform makes this card its own stacking context.
        <div class="group relative flex flex-col rounded-xl border border-border bg-card text-card-foreground shadow-sm transition-all focus-within:z-20 hover:-translate-y-0.5 hover:shadow-md">
            // Banner + avatar
            <div class="relative h-16 rounded-t-xl bg-gradient-to-r from-muted to-accent">
                {if has_avatar {
                    view! {
                        <img
                            src=avatar_url.clone().unwrap_or_default()
                            alt=""
                            class="absolute -bottom-7 left-5 h-14 w-14 rounded-full object-cover shadow-md ring-4 ring-card"
                        />
                    }.into_any()
                } else {
                    view! {
                        <div class=format!(
                            "absolute -bottom-7 left-5 flex h-14 w-14 items-center justify-center rounded-full bg-gradient-to-br {} text-xl font-semibold text-white shadow-md ring-4 ring-card",
                            gradient
                        )>
                            {av_initial}
                        </div>
                    }.into_any()
                }}
            </div>

            <div class="flex flex-1 flex-col p-5 pt-9">
                <h3 class="truncate text-base font-semibold" title=name.clone()>{name.clone()}</h3>
                {role.map(|r| view! {
                    <p class="mt-0.5 truncate text-sm text-muted-foreground">{r}</p>
                })}

                <p class="mt-3 line-clamp-3 min-h-[3.75rem] text-sm text-muted-foreground">
                    {personality.unwrap_or_else(|| "No personality description yet.".to_string())}
                </p>

                <div class="mt-4 flex items-center gap-2">
                    <button
                        class="inline-flex h-9 flex-1 items-center justify-center gap-1.5 rounded-lg bg-primary px-3 text-sm font-medium text-primary-foreground shadow transition-colors hover:bg-primary/90"
                        on:click=move |_| on_start.run(id)
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7.9 20A9 9 0 1 0 4 16.1L2 22Z"/></svg>
                        "Chat"
                    </button>
                    <button
                        class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-input bg-background text-muted-foreground shadow-sm transition-colors hover:bg-accent hover:text-foreground"
                        title="Edit"
                        aria-label="Edit character"
                        on:click=move |_| on_edit.run(for_edit.clone())
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.12 2.12 0 0 1 3 3L12 15l-4 1 1-4Z"/></svg>
                    </button>
                    <DropdownMenu>
                        <DropdownMenuTrigger class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-input \
                                                     bg-background text-muted-foreground shadow-sm transition-colors \
                                                     hover:bg-accent hover:text-foreground">
                            <span class="sr-only">"More actions"</span>
                            <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/></svg>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align=DropdownAlign::End>
                            {
                                let for_duplicate = for_duplicate.clone();
                                let for_export = for_export.clone();
                                let for_delete = for_delete.clone();
                                view! {
                                    <DropdownMenuItem on_select=Callback::new(move |_| on_duplicate.run(for_duplicate.clone()))>
                                        <span class="flex items-center gap-2">
                                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>
                                            "Duplicate"
                                        </span>
                                    </DropdownMenuItem>
                                    <DropdownMenuItem on_select=Callback::new(move |_| on_export.run(for_export.clone()))>
                                        <span class="flex items-center gap-2">
                                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 15V3"/><path d="m7 10 5 5 5-5"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/></svg>
                                            "Export JSON"
                                        </span>
                                    </DropdownMenuItem>
                                    <DropdownMenuSeparator />
                                    <DropdownMenuItem
                                        class="text-destructive focus:bg-destructive focus:text-destructive-foreground hover:bg-destructive hover:text-destructive-foreground"
                                        on_select=Callback::new(move |_| on_delete.run(for_delete.clone()))
                                    >
                                        <span class="flex items-center gap-2">
                                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
                                            "Delete"
                                        </span>
                                    </DropdownMenuItem>
                                }
                            }
                        </DropdownMenuContent>
                    </DropdownMenu>
                </div>
            </div>
        </div>
    }
}

#[component]
fn CharactersEmpty(on_create: Callback<()>, on_import: Callback<()>) -> impl IntoView {
    view! {
        <div class="flex flex-col items-center justify-center rounded-2xl border border-dashed border-border py-20 text-center">
            <div class="mb-4 flex h-16 w-16 items-center justify-center rounded-2xl bg-gradient-to-br from-cyan-500 to-blue-600 text-white shadow-lg">
                <svg xmlns="http://www.w3.org/2000/svg" width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></svg>
            </div>
            <h3 class="text-lg font-semibold">"Create your first character"</h3>
            <p class="mt-1.5 max-w-sm text-sm text-muted-foreground">
                "Characters are the personas you roleplay with. Give one a name and personality to start chatting."
            </p>
            <div class="mt-6 flex items-center gap-2">
                <button class=BTN_PRIMARY on:click=move |_| on_create.run(())>
                    <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5v14"/></svg>
                    "New character"
                </button>
                <button class=BTN_OUTLINE on:click=move |_| on_import.run(())>
                    "Import JSON"
                </button>
            </div>
        </div>
    }
}

#[component]
fn CharactersSkeleton() -> impl IntoView {
    view! {
        <div class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {(0..4).map(|_| view! {
                <div class="overflow-hidden rounded-xl border border-border bg-card shadow-sm">
                    <div class="h-16 animate-pulse bg-muted"></div>
                    <div class="p-5 pt-9">
                        <div class="h-4 w-1/2 animate-pulse rounded bg-muted"></div>
                        <div class="mt-2 h-3 w-1/3 animate-pulse rounded bg-muted"></div>
                        <div class="mt-4 h-3 w-full animate-pulse rounded bg-muted"></div>
                        <div class="mt-2 h-3 w-4/5 animate-pulse rounded bg-muted"></div>
                        <div class="mt-5 h-9 w-full animate-pulse rounded-lg bg-muted"></div>
                    </div>
                </div>
            }).collect::<Vec<_>>()}
        </div>
    }
}
