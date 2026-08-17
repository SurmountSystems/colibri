//! Lightweight string tables for colibri-native (English + Italian).
//!
//! Mirrors keys used by `web/src/i18n/*` for shared product copy. Native-only
//! lifecycle strings (Machine, Doctor, Plan, Install, Start engine) live here
//! too so the rail can switch locale without hard-coded English.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Supported UI locales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    #[default]
    En,
    It,
}

impl Locale {
    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::It => "it",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::It => "Italiano",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Locale::En => Locale::It,
            Locale::It => Locale::En,
        }
    }
}

/// Look up a key in the active locale; falls back to English, then the key.
pub fn t(locale: Locale, key: &str) -> String {
    if let Some(s) = table(locale).get(key) {
        return s.clone();
    }
    if locale != Locale::En {
        if let Some(s) = table(Locale::En).get(key) {
            return s.clone();
        }
    }
    key.to_string()
}

/// Simple `{{name}}` substitution (web-style placeholders).
pub fn t_fmt(locale: Locale, key: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = t(locale, key);
    for (name, value) in pairs {
        out = out.replace(&format!("{{{{{name}}}}}"), value);
    }
    out
}

fn table(locale: Locale) -> &'static HashMap<&'static str, String> {
    match locale {
        Locale::En => en_table(),
        Locale::It => it_table(),
    }
}

fn en_table() -> &'static HashMap<&'static str, String> {
    static T: OnceLock<HashMap<&'static str, String>> = OnceLock::new();
    T.get_or_init(|| {
        let mut m = HashMap::new();
        for &(k, v) in EN {
            m.insert(k, v.to_string());
        }
        m
    })
}

fn it_table() -> &'static HashMap<&'static str, String> {
    static T: OnceLock<HashMap<&'static str, String>> = OnceLock::new();
    T.get_or_init(|| {
        let mut m = HashMap::new();
        for &(k, v) in IT {
            m.insert(k, v.to_string());
        }
        m
    })
}

const EN: &[(&str, &str)] = &[
    // nav
    ("nav.chat", "Chat"),
    ("nav.brain", "Brain"),
    ("nav.profiling", "Profiling"),
    ("nav.tools", "Tools"),
    // brand
    ("brand.name", "colibrì"),
    ("brand.tagline", "local giant, tiny footprint"),
    ("brand.native", "native"),
    // lifecycle rail (slim)
    ("rail.machine", "Machine"),
    ("rail.doctor", "Check model"),
    ("rail.plan", "Memory plan"),
    ("rail.inference", "Chat settings"),
    ("rail.install", "Download model"),
    ("rail.runtime", "Live placement"),
    ("rail.lifecycle", "Engine"),
    ("rail.details", "Details"),
    ("rail.hideDetails", "Hide details"),
    ("rail.refresh", "Refresh"),
    ("rail.runChecks", "Run checks"),
    ("rail.deepCheck", "Thorough check"),
    ("rail.planBtn", "Plan memory"),
    ("rail.startEngine", "Start engine"),
    ("rail.stopEngine", "Stop engine"),
    ("rail.scanModels", "Scan models"),
    ("rail.installBtn", "Install model"),
    ("rail.installing", "Installing…"),
    // native-only operational (install pause/resume; no SPA source)
    ("rail.pause", "Pause"),
    ("rail.resume", "Resume"),
    ("rail.pausing", "Pausing…"),
    ("rail.cancel", "Cancel"),
    ("rail.locale", "Language"),
    ("rail.about", "About"),
    ("rail.hideAbout", "Hide about"),
    ("rail.setup", "Setup"),
    ("rail.modelPath", "Model folder"),
    ("rail.modelUnset", "No model selected"),
    // tiers
    ("tier.vram", "Memory on GPU"),
    ("tier.ram", "System RAM"),
    ("tier.disk", "Disk"),
    ("tier.title", "Where experts live"),
    // inference
    ("sidebar.temperature", "Temperature"),
    ("sidebar.maxTokens", "Max output tokens"),
    ("sidebar.reasoning", "Reasoning"),
    ("sidebar.reasoningOn", "Reasoning: on"),
    ("sidebar.reasoningOff", "Reasoning: off"),
    ("sidebar.kvSession", "Chat session"),
    ("sidebar.sessionLabel", "Session {{slot}} of {{n}}"),
    (
        "sidebar.sessionHelp",
        "Conversation follows the selected session slot.",
    ),
    ("sidebar.grammar", "Grammar (optional)"),
    ("sidebar.prev", "Prev"),
    ("sidebar.next", "Next"),
    // topbar
    ("topbar.activeModel", "ACTIVE MODEL"),
    ("topbar.tokens", "{{n}} tokens"),
    ("topbar.tokPerSec", "{{n}} tok/s"),
    ("topbar.ttft", "TTFT {{n}} ms"),
    ("topbar.slot", "slot {{n}}"),
    ("topbar.clear", "Clear"),
    // hero
    ("hero.title", "COLIBRÌ ENGINE"),
    ("hero.subtitle", "Ask the giant."),
    ("hero.tagline", "Keep the machine yours."),
    (
        "hero.description",
        "Run a local model on this machine without a browser. Use Setup if you are new, or pick a model folder and start the engine, then chat.",
    ),
    (
        "hero.setupHint",
        "First time here? Open Setup for a short guided path.",
    ),
    (
        "hero.nextNeedModel",
        "Pick or install a model in Tools or the left rail, then Start engine.",
    ),
    (
        "hero.nextStartEngine",
        "Start engine in the left rail, then chat.",
    ),
    // HF install form
    ("install.repo", "Hugging Face repo (owner/name)"),
    ("install.revision", "Revision (optional)"),
    ("install.dest", "Folder name under store (optional)"),
    ("install.minFree", "Min free disk (GB)"),
    (
        "install.minFreeHelp",
        "Default 1. Set 0 to turn the free-space check off.",
    ),
    ("prompts.routing", "Explain how expert routing works"),
    ("prompts.benchmark", "Write a small C benchmark"),
    ("prompts.caching", "Compare RAM and GPU memory caching"),
    // chat
    ("chat.you", "You"),
    ("chat.colibri", "colibrì"),
    ("chat.placeholder", "Message colibrì…"),
    ("chat.send", "Send"),
    ("chat.stop", "Stop"),
    ("chat.inputHint", "Type a message, then Send"),
    // brain
    ("brain.title", "Expert Cortex"),
    (
        "brain.waiting",
        "Start the engine and send a message to see expert activity.",
    ),
    ("brain.layers", "{{rows}} × {{cols}}"),
    ("brain.brightnessHint", "brightness = routing heat"),
    ("brain.flashHint", "flash = routed this turn"),
    // Theme-aware legends (mint continuous map vs DOGE discrete eight).
    (
        "brain.legend.mint",
        "Gray = disk · Blue = system RAM · Green = GPU · Bright = hot · Flash = hit",
    ),
    (
        "brain.legend.doge",
        "Black = cold · Blue = disk warm · Cyan = RAM warm · Green = GPU · Yellow = GPU hot · Magenta = hot · White = hit",
    ),
    // Fallback key (mint-shaped) for callers that have not switched yet.
    (
        "brain.legend",
        "Gray = disk · Blue = system RAM · Green = GPU · Bright = hot · Flash = hit",
    ),
    // profiling
    (
        "profile.title",
        "Profiling: where the engine spends each turn",
    ),
    ("profile.ioWait", "I/O wait"),
    ("profile.expertMatmul", "Expert matmul"),
    ("profile.attention", "Attention"),
    ("profile.lmHead", "LM head"),
    ("profile.other", "Other"),
    (
        "profile.empty",
        "No profiled turns yet. Send a chat message and the breakdown appears here.",
    ),
    (
        "profile.connectHint",
        "Start the engine to collect per-turn timings.",
    ),
    ("profile.lastTurn", "Last turn"),
    ("profile.wallTime", "Wall time"),
    ("profile.batching", "Batching"),
    ("profile.tokensPerForward", "tokens / forward"),
    ("profile.diskService", "Disk service"),
    ("profile.overlapped", "overlapped with compute"),
    ("profile.window", "Window · last {{n}} turns"),
    ("profile.throughputTitle", "Throughput per turn (tok/s)"),
    ("profile.phaseTitle", "Turn wall time by phase (s)"),
    ("profile.turnCol", "Turn"),
    ("profile.tokensCol", "Tokens"),
    ("profile.wallCol", "Wall"),
    ("profile.turnsLabel", "{{n}} turns · oldest → newest"),
    ("profile.oneTurn", "1 turn"),
    (
        "profile.diskNote",
        "Disk service is time spent reading experts on I/O threads; it overlaps with compute, so only the I/O wait the compute thread felt counts inside the wall-time stack.",
    ),
    // Tools panel
    ("tools.title", "Tools"),
    ("tools.subtitle", "Machine checks, models, look and feel."),
    ("tools.machine", "Your machine"),
    ("tools.doctor", "Check model"),
    ("tools.plan", "Memory plan"),
    ("tools.scan", "Scan models"),
    ("tools.install", "Download model"),
    ("tools.theme", "Looks"),
    ("tools.language", "Language"),
    ("tools.about", "About"),
    ("tools.advanced", "Advanced chat options"),
    ("tools.modelPath", "Model folder path"),
    // Theme
    ("theme.label", "Looks"),
    ("theme.doge", "DOGE"),
    ("theme.mint", "Mint"),
    ("theme.dogeHelp", "High-contrast pure colors (default)."),
    ("theme.mintHelp", "Softer green dashboard look."),
    // Setup / wizard
    ("setup.open", "Setup"),
    ("setup.reopen", "Open setup again anytime from Tools."),
    ("wizard.back", "Back"),
    ("wizard.next", "Next"),
    ("wizard.skip", "Skip"),
    ("wizard.finish", "Finish"),
    ("wizard.stepOf", "Step {{n}} of {{total}}"),
    ("wizard.welcome.title", "Welcome"),
    (
        "wizard.welcome.body",
        "colibrì runs large models on this computer. This short setup helps you check the machine, pick a model, and choose how the app looks.",
    ),
    ("wizard.machine.title", "Your machine"),
    (
        "wizard.machine.body",
        "CPU, memory, and graphics we found. Refresh if you plugged in a GPU or changed hardware.",
    ),
    ("wizard.model.title", "Choose a model"),
    (
        "wizard.model.body",
        "Paste a folder path, pick from the model store, or download from Hugging Face.",
    ),
    ("wizard.model.path", "Model folder"),
    ("wizard.model.downloadShow", "Show download options"),
    ("wizard.model.downloadHide", "Hide download options"),
    ("catalog.supported", "Supported models"),
    (
        "catalog.supportedHelp",
        "Pick a model Colibri supports. Install fills the download form when a Hugging Face snapshot is available.",
    ),
    // Native-only operational badge on Supported models rows (Present on disk).
    ("catalog.installed", "Installed"),
    ("wizard.readiness.title", "Doctor"),
    (
        "wizard.readiness.body",
        "Run doctor on this path, then review the short memory plan. Fix anything red before you start the engine.",
    ),
    ("wizard.readiness.doctor", "Health check"),
    ("wizard.readiness.plan", "Memory plan"),
    ("wizard.readiness.refresh", "Quick check"),
    ("wizard.readiness.runDoctor", "Run doctor"),
    ("wizard.readiness.scan", "Scan for models"),
    ("wizard.readiness.install", "Install a model"),
    (
        "wizard.readiness.actionsHint",
        "Run doctor checks this path. Scan looks under the default store. Install downloads a model.",
    ),
    ("wizard.look.title", "Look and feel"),
    (
        "wizard.look.body",
        "Pick a color theme. You can change this later under Tools.",
    ),
    ("wizard.ready.title", "Ready"),
    (
        "wizard.ready.body",
        "You are set. Finish to use the dashboard. Starting the engine is optional.",
    ),
    ("wizard.ready.summaryTheme", "Theme"),
    ("wizard.ready.summaryModel", "Model"),
    ("wizard.ready.summaryLocale", "Language"),
    ("wizard.ready.start", "Start engine"),
    (
        "wizard.ready.modelNotReady",
        "This path is not a model yet. Install a model or choose a folder with config.json and weights.",
    ),
];

const IT: &[(&str, &str)] = &[
    ("nav.chat", "Chat"),
    ("nav.brain", "Cervello"),
    ("nav.profiling", "Profiling"),
    ("nav.tools", "Strumenti"),
    ("brand.name", "colibrì"),
    ("brand.tagline", "gigante locale, impronta minima"),
    ("brand.native", "nativo"),
    ("rail.machine", "Macchina"),
    ("rail.doctor", "Controlla modello"),
    ("rail.plan", "Piano memoria"),
    ("rail.inference", "Impostazioni chat"),
    ("rail.install", "Scarica modello"),
    ("rail.runtime", "Collocazione live"),
    ("rail.lifecycle", "Motore"),
    ("rail.details", "Dettagli"),
    ("rail.hideDetails", "Nascondi dettagli"),
    ("rail.refresh", "Aggiorna"),
    ("rail.runChecks", "Controlli"),
    ("rail.deepCheck", "Controllo completo"),
    ("rail.planBtn", "Piano memoria"),
    ("rail.startEngine", "Avvia motore"),
    ("rail.stopEngine", "Ferma motore"),
    ("rail.scanModels", "Scansiona modelli"),
    ("rail.installBtn", "Installa modello"),
    ("rail.installing", "Installazione…"),
    // native-only operational (install pause/resume)
    ("rail.pause", "Pausa"),
    ("rail.resume", "Riprendi"),
    ("rail.pausing", "In pausa…"),
    ("rail.cancel", "Annulla"),
    ("rail.locale", "Lingua"),
    ("rail.about", "Info"),
    ("rail.hideAbout", "Nascondi info"),
    ("rail.setup", "Configura"),
    ("rail.modelPath", "Cartella modello"),
    ("rail.modelUnset", "Nessun modello selezionato"),
    ("tier.vram", "Memoria GPU"),
    ("tier.ram", "RAM di sistema"),
    ("tier.disk", "Disco"),
    ("tier.title", "Dove vivono gli expert"),
    ("sidebar.temperature", "Temperatura"),
    ("sidebar.maxTokens", "Token di output massimi"),
    ("sidebar.reasoning", "Ragionamento"),
    ("sidebar.reasoningOn", "Ragionamento: on"),
    ("sidebar.reasoningOff", "Ragionamento: off"),
    ("sidebar.kvSession", "Sessione chat"),
    ("sidebar.sessionLabel", "Sessione {{slot}} di {{n}}"),
    (
        "sidebar.sessionHelp",
        "La conversazione segue lo slot di sessione selezionato.",
    ),
    ("sidebar.grammar", "Grammatica (opzionale)"),
    ("sidebar.prev", "Prec"),
    ("sidebar.next", "Succ"),
    ("topbar.activeModel", "MODELLO ATTIVO"),
    ("topbar.tokens", "{{n}} token"),
    ("topbar.tokPerSec", "{{n}} tok/s"),
    ("topbar.ttft", "TTFT {{n}} ms"),
    ("topbar.slot", "slot {{n}}"),
    ("topbar.clear", "Pulisci"),
    ("hero.title", "MOTORE COLIBRÌ"),
    ("hero.subtitle", "Interroga il gigante."),
    ("hero.tagline", "La macchina resta tua."),
    (
        "hero.description",
        "Esegui un modello locale su questa macchina senza browser. Usa Configura se sei alle prime armi, oppure scegli una cartella modello e avvia il motore, poi chatta.",
    ),
    (
        "hero.setupHint",
        "Prima volta? Apri Configura per un percorso guidato breve.",
    ),
    (
        "hero.nextNeedModel",
        "Scegli o installa un modello in Strumenti o nella barra sinistra, poi Avvia motore.",
    ),
    (
        "hero.nextStartEngine",
        "Avvia il motore nella barra sinistra, poi chatta.",
    ),
    ("install.repo", "Repo Hugging Face (owner/name)"),
    ("install.revision", "Revisione (opzionale)"),
    ("install.dest", "Nome cartella nello store (opzionale)"),
    ("install.minFree", "Spazio libero minimo (GB)"),
    (
        "install.minFreeHelp",
        "Predefinito 1. Imposta 0 per disattivare il controllo di spazio libero.",
    ),
    (
        "prompts.routing",
        "Spiega come funziona il routing degli expert",
    ),
    ("prompts.benchmark", "Scrivi un piccolo benchmark in C"),
    ("prompts.caching", "Confronta il caching RAM e memoria GPU"),
    ("chat.you", "Tu"),
    ("chat.colibri", "colibrì"),
    ("chat.placeholder", "Scrivi a colibrì…"),
    ("chat.send", "Invia"),
    ("chat.stop", "Ferma"),
    ("chat.inputHint", "Scrivi un messaggio, poi Invia"),
    ("brain.title", "Corteccia degli expert"),
    (
        "brain.waiting",
        "Avvia il motore e invia un messaggio per vedere l'attività degli expert.",
    ),
    ("brain.layers", "{{rows}} × {{cols}}"),
    ("brain.brightnessHint", "luminosità = calore di routing"),
    ("brain.flashHint", "flash = instradato in questo turno"),
    (
        "brain.legend.mint",
        "Grigio = disco · Blu = RAM di sistema · Verde = GPU · Chiaro = caldo · Flash = hit",
    ),
    (
        "brain.legend.doge",
        "Nero = freddo · Blu = disco tiepido · Ciano = RAM tiepida · Verde = GPU · Giallo = GPU calda · Magenta = caldo · Bianco = hit",
    ),
    (
        "brain.legend",
        "Grigio = disco · Blu = RAM di sistema · Verde = GPU · Chiaro = caldo · Flash = hit",
    ),
    (
        "profile.title",
        "Profiling: dove il motore spende ogni turno",
    ),
    ("profile.ioWait", "Attesa I/O"),
    ("profile.expertMatmul", "Matmul expert"),
    ("profile.attention", "Attention"),
    ("profile.lmHead", "LM head"),
    ("profile.other", "Altro"),
    (
        "profile.empty",
        "Nessun turno profilato. Invia un messaggio in chat e la scomposizione appare qui.",
    ),
    (
        "profile.connectHint",
        "Avvia il motore per raccogliere i tempi per turno.",
    ),
    ("profile.lastTurn", "Ultimo turno"),
    ("profile.wallTime", "Tempo wall"),
    ("profile.batching", "Batching"),
    ("profile.tokensPerForward", "token / forward"),
    ("profile.diskService", "Servizio disco"),
    ("profile.overlapped", "sovrapposto al compute"),
    ("profile.window", "Finestra · ultimi {{n}} turni"),
    ("profile.throughputTitle", "Throughput per turno (tok/s)"),
    ("profile.phaseTitle", "Tempo wall per fase (s)"),
    ("profile.turnCol", "Turno"),
    ("profile.tokensCol", "Token"),
    ("profile.wallCol", "Wall"),
    (
        "profile.turnsLabel",
        "{{n}} turni · più vecchio → più recente",
    ),
    ("profile.oneTurn", "1 turno"),
    (
        "profile.diskNote",
        "Il servizio disco è il tempo speso a leggere expert su thread I/O; si sovrappone al compute, quindi solo l'attesa I/O sentita dal thread di compute conta nello stack wall-time.",
    ),
    ("tools.title", "Strumenti"),
    ("tools.subtitle", "Controlli macchina, modelli, aspetto."),
    ("tools.machine", "La tua macchina"),
    ("tools.doctor", "Controlla modello"),
    ("tools.plan", "Piano memoria"),
    ("tools.scan", "Scansiona modelli"),
    ("tools.install", "Scarica modello"),
    ("tools.theme", "Aspetto"),
    ("tools.language", "Lingua"),
    ("tools.about", "Info"),
    ("tools.advanced", "Opzioni chat avanzate"),
    ("tools.modelPath", "Percorso cartella modello"),
    ("theme.label", "Aspetto"),
    ("theme.doge", "DOGE"),
    ("theme.mint", "Menta"),
    (
        "theme.dogeHelp",
        "Colori puri ad alto contrasto (predefinito).",
    ),
    ("theme.mintHelp", "Aspetto dashboard più morbido e verde."),
    ("setup.open", "Configura"),
    (
        "setup.reopen",
        "Puoi riaprire la configurazione in qualsiasi momento da Strumenti.",
    ),
    ("wizard.back", "Indietro"),
    ("wizard.next", "Avanti"),
    ("wizard.skip", "Salta"),
    ("wizard.finish", "Fine"),
    ("wizard.stepOf", "Passo {{n}} di {{total}}"),
    ("wizard.welcome.title", "Benvenuto"),
    (
        "wizard.welcome.body",
        "colibrì esegue modelli grandi su questo computer. Questa breve configurazione ti aiuta a controllare la macchina, scegliere un modello e l'aspetto dell'app.",
    ),
    ("wizard.machine.title", "La tua macchina"),
    (
        "wizard.machine.body",
        "CPU, memoria e grafica trovate. Aggiorna se hai collegato una GPU o cambiato hardware.",
    ),
    ("wizard.model.title", "Scegli un modello"),
    (
        "wizard.model.body",
        "Incolla il percorso di una cartella, scegli dallo store, oppure scarica da Hugging Face.",
    ),
    ("wizard.model.path", "Cartella modello"),
    ("wizard.model.downloadShow", "Mostra opzioni di download"),
    ("wizard.model.downloadHide", "Nascondi opzioni di download"),
    ("catalog.supported", "Modelli supportati"),
    (
        "catalog.supportedHelp",
        "Scegli un modello supportato da Colibri. Installa compila il modulo di download quando è disponibile uno snapshot Hugging Face.",
    ),
    ("catalog.installed", "Installato"),
    ("wizard.readiness.title", "Doctor"),
    (
        "wizard.readiness.body",
        "Esegui doctor su questo percorso, poi rivedi il piano memoria. Risolvi ciò che è in rosso prima di avviare il motore.",
    ),
    ("wizard.readiness.doctor", "Controllo salute"),
    ("wizard.readiness.plan", "Piano memoria"),
    ("wizard.readiness.refresh", "Controllo rapido"),
    ("wizard.readiness.runDoctor", "Esegui doctor"),
    ("wizard.readiness.scan", "Cerca modelli"),
    ("wizard.readiness.install", "Installa un modello"),
    (
        "wizard.readiness.actionsHint",
        "Esegui doctor controlla questo percorso. Cerca guarda nello store predefinito. Installa scarica un modello.",
    ),
    ("wizard.look.title", "Aspetto"),
    (
        "wizard.look.body",
        "Scegli un tema colori. Potrai cambiarlo dopo in Strumenti.",
    ),
    ("wizard.ready.title", "Pronto"),
    (
        "wizard.ready.body",
        "Tutto a posto. Premi Fine per usare la dashboard. Avviare il motore è opzionale.",
    ),
    ("wizard.ready.summaryTheme", "Tema"),
    ("wizard.ready.summaryModel", "Modello"),
    ("wizard.ready.summaryLocale", "Lingua"),
    ("wizard.ready.start", "Avvia motore"),
    (
        "wizard.ready.modelNotReady",
        "Questo percorso non è ancora un modello. Installa un modello o scegli una cartella con config.json e i pesi.",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_nav_keys() {
        assert_eq!(t(Locale::En, "nav.chat"), "Chat");
        assert_eq!(t(Locale::En, "nav.brain"), "Brain");
        assert_eq!(t(Locale::En, "nav.profiling"), "Profiling");
        assert_eq!(t(Locale::En, "nav.tools"), "Tools");
        assert_eq!(
            t(Locale::En, "brand.tagline"),
            "local giant, tiny footprint"
        );
    }

    #[test]
    fn italian_nav_keys() {
        assert_eq!(t(Locale::It, "nav.brain"), "Cervello");
        assert_eq!(t(Locale::It, "nav.tools"), "Strumenti");
        assert_eq!(t(Locale::It, "hero.subtitle"), "Interroga il gigante.");
        assert_eq!(t(Locale::It, "chat.send"), "Invia");
    }

    #[test]
    fn setup_reopen_hint_points_at_tools_not_only_rail() {
        let en = t(Locale::En, "setup.reopen");
        let it = t(Locale::It, "setup.reopen");
        let en_l = en.to_ascii_lowercase();
        assert!(
            en_l.contains("tools"),
            "native reopen hint must name Tools: {en}"
        );
        assert!(
            !en_l.contains("left rail"),
            "left rail is no longer the reopen path: {en}"
        );
        assert!(
            it.to_ascii_lowercase().contains("strumenti"),
            "Italian reopen hint must name Strumenti: {it}"
        );
    }

    #[test]
    fn wizard_and_tools_keys_en_it() {
        for key in [
            "wizard.welcome.title",
            "wizard.readiness.title",
            "wizard.readiness.runDoctor",
            "wizard.readiness.refresh",
            "wizard.readiness.scan",
            "wizard.readiness.install",
            "wizard.readiness.actionsHint",
            "wizard.finish",
            "tools.title",
            "theme.doge",
            "theme.mint",
            "rail.setup",
            "rail.stopEngine",
        ] {
            assert_ne!(t(Locale::En, key), key, "{key}");
            assert_ne!(t(Locale::It, key), key, "{key}");
        }
        assert_eq!(t(Locale::En, "theme.doge"), "DOGE");
        assert_eq!(t(Locale::En, "theme.mint"), "Mint");
        // Wizard step 4 is the Doctor step; primary CTA is a verb phrase.
        assert_eq!(t(Locale::En, "wizard.readiness.title"), "Doctor");
        assert_eq!(t(Locale::It, "wizard.readiness.title"), "Doctor");
        assert_eq!(t(Locale::En, "wizard.readiness.runDoctor"), "Run doctor");
        assert_eq!(t(Locale::It, "wizard.readiness.runDoctor"), "Esegui doctor");
        assert_eq!(t(Locale::En, "wizard.readiness.refresh"), "Quick check");
        assert_eq!(
            t(Locale::It, "wizard.readiness.refresh"),
            "Controllo rapido"
        );
        assert_eq!(t(Locale::En, "wizard.readiness.scan"), "Scan for models");
        assert_eq!(t(Locale::En, "wizard.readiness.install"), "Install a model");
        assert!(
            t(Locale::En, "wizard.readiness.actionsHint")
                .to_ascii_lowercase()
                .contains("scan")
        );
        assert!(!t(Locale::En, "wizard.readiness.actionsHint").contains("COLIBRI_"));
    }

    #[test]
    fn fallback_to_english_for_missing_it() {
        // All keys exist in both; missing key returns key name.
        assert_eq!(t(Locale::It, "does.not.exist"), "does.not.exist");
    }

    #[test]
    fn fmt_placeholder() {
        let s = t_fmt(Locale::En, "topbar.tokens", &[("n", "42")]);
        assert_eq!(s, "42 tokens");
        let s = t_fmt(Locale::It, "topbar.tokPerSec", &[("n", "12.5")]);
        assert_eq!(s, "12.5 tok/s");
    }

    #[test]
    fn locale_cycle() {
        assert_eq!(Locale::En.next(), Locale::It);
        assert_eq!(Locale::It.next(), Locale::En);
        assert_eq!(Locale::En.code(), "en");
        assert_eq!(Locale::It.label(), "Italiano");
    }

    #[test]
    fn en_table_has_core_surface() {
        for key in [
            "hero.title",
            "profile.ioWait",
            "tier.vram",
            "rail.startEngine",
            "brain.title",
            "install.minFree",
            "install.repo",
            "install.minFreeHelp",
            "nav.tools",
            "wizard.welcome.body",
            "tools.subtitle",
            "catalog.supported",
            "catalog.supportedHelp",
            "catalog.installed",
        ] {
            assert!(!t(Locale::En, key).is_empty(), "{key}");
            assert_ne!(t(Locale::En, key), key, "{key}");
        }
        assert_eq!(t(Locale::En, "catalog.installed"), "Installed");
        assert_eq!(t(Locale::It, "catalog.installed"), "Installato");
    }

    #[test]
    fn install_labels_en_and_it() {
        assert_eq!(t(Locale::En, "install.minFree"), "Min free disk (GB)");
        assert!(
            t(Locale::En, "install.minFreeHelp").contains("0"),
            "{}",
            t(Locale::En, "install.minFreeHelp")
        );
        assert_ne!(t(Locale::It, "install.minFree"), "install.minFree");
        assert_ne!(t(Locale::It, "install.repo"), "install.repo");
        assert!(t(Locale::It, "install.minFree").contains("GB"));
    }
}
