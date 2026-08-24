/**
 * German.
 *
 * Typed against `en`, so a key added there and missed here fails the build.
 * German pluralises like English — one/other — so no `few`/`many` forms.
 */
import type { en } from "./en";
import type { Phrase } from "./index";

export const de: Record<keyof typeof en, Phrase> = {
  // ---------------------------------------------------------------- toolbar
  "toolbar.sources": "Quellen",
  "toolbar.addFolders": "Ordner hinzufügen",
  "toolbar.addFiles": "Dateien hinzufügen",
  "toolbar.remove": "Entfernen",
  "toolbar.clear": "Leeren",
  "toolbar.plan": "Plan",
  "toolbar.planSave": "Speichern",
  "toolbar.planOpen": "Öffnen",
  "toolbar.sort": "Sortierung",
  "toolbar.sortName": "Name",
  "toolbar.sortSize": "Größe",
  "toolbar.selection": "Auswahl",
  "toolbar.checkAll": "Alle auswählen",
  "toolbar.uncheckAll": "Auswahl aufheben",
  "toolbar.collapseAll": "Alle einklappen",
  "toolbar.reading": "Wird gelesen…",
  "toolbar.stop": "Anhalten",
  "toolbar.scanned": "{files} Dateien · {bytes}",
  "toolbar.settings": "Einstellungen",
  "toolbar.about": "Über Tree Archiver",

  // ---------------------------------------------------------------- theme and language
  "theme.label": "Design: {name}",
  "theme.system": "System",
  "theme.systemLong": "System (folgt Windows)",
  "theme.light": "Hell",
  "theme.dark": "Dunkel",
  "lang.label": "Sprache: {name}",
  "lang.system": "System",
  "lang.systemLong": "System (folgt Windows)",

  // ---------------------------------------------------------------- tree
  "tree.emptyTitle": "Noch nichts vorgemerkt",
  "tree.emptyBody":
    "Ziehen Sie Ordner an eine beliebige Stelle dieses Fensters oder fügen Sie sie über die Symbolleiste hinzu. Alles Hinzugefügte ist zunächst vollständig ausgewählt — heben Sie die Auswahl für alles auf, was ausgelassen werden soll.",
  "tree.expand": "Ausklappen",
  "tree.collapse": "Einklappen",
  "tree.passThrough": "Durchgang",
  "tree.reading": "wird gelesen…",
  "tree.dropHere": "Loslassen, um diese Pfade vorzumerken",

  // ---------------------------------------------------------------- status bar
  "status.sources": "Quellen",
  "status.files": "Dateien",
  "status.selected": "Ausgewählt",
  "status.ofFiles": "{sel} von {total}",
  "status.ofBytes": "von {total}",
  "status.idle": "Keine Quellen vorgemerkt",
  "status.archive": "Archivieren…",
  "status.archiveReady": "Zieldatei wählen und Archiv erstellen",
  "status.archiveEmpty": "Zuerst etwas auswählen",
  "status.unreadable": {
    one: "{count} Pfad konnte nicht gelesen werden",
    other: "{count} Pfade konnten nicht gelesen werden",
  },

  // ---------------------------------------------------------------- build dialog
  "build.title": "Archiv erstellen",
  "build.output": "Zieldatei",
  "build.browse": "Durchsuchen…",
  "build.paths": "Pfade im Archiv",
  "build.modeFoldersOnly": "Nur Ordner",
  "build.modeCommonRoot": "Gemeinsamer Ordner",
  "build.modeFullPath": "Vollständiger Pfad",
  "build.blurbFoldersOnly": "Jeder vorgemerkte Ordner liegt oben im Archiv.",
  "build.blurbCommonRoot": "Behält den Ordner, den die Pfade gemeinsam haben.",
  "build.blurbFullPath": "Behält den ganzen Pfad einschließlich Laufwerksbuchstabe.",
  "build.unavailable": "Nicht verfügbar — {reason}.",
  "build.notUsable": "hier nicht verwendbar",
  "build.compression": "Komprimierung",
  "build.compressionNone": "Keine (.tar)",
  "build.compressionGzip": "gzip (.tar.gz)",
  "build.compression7z": "7z (.7z)",
  "build.solid": "Solides Archiv",
  "build.solidHint":
    "Packt alle Dateien in einen gemeinsamen Datenstrom. Kleiner bei vielen kleinen Dateien, dafür gröberer Fortschritt und langsameres Entpacken einzelner Dateien.",
  "build.level": "Stufe {level}",
  "build.faster": "schneller",
  "build.smaller": "kleiner",
  "build.spec": "Archivdaten",
  "build.entries": "Einträge",
  "build.files": "Dateien",
  "build.content": "Inhalt",
  "build.maxSize": "Maximalgröße",
  "build.archiveSize": "Archivgröße",
  "build.beforeCompression": "vor der Komprimierung",
  "build.exact": "exakt",
  "build.cancel": "Abbrechen",
  "build.start": "Starten",
  "build.starting": "Wird gestartet…",
  "build.pickOutputFirst": "Wählen Sie zuerst, wo das Archiv gespeichert werden soll.",
  "build.saveAs": "Archiv speichern unter",
  "build.filterTar": "Tar-Archiv",
  "build.filterTarGz": "Gzip-Tar-Archiv",
  "build.filter7z": "7z-Archiv",

  // ---------------------------------------------------------------- progress
  "progress.building": "Archiv wird erstellt",
  "progress.built": "Archiv erstellt",
  "progress.builtWarnings": "Archiv mit Warnungen erstellt",
  "progress.failed": "Archiv fehlgeschlagen",
  "progress.cancelled": "Archivierung abgebrochen",
  "progress.files": "Dateien",
  "progress.written": "Geschrieben",
  "progress.ofBytes": "von {total}",
  "progress.rate": "Tempo",
  "progress.elapsed": "Dauer",
  "progress.remaining": "Verbleibend",
  "progress.cancelledNote": "Abgebrochen. Das unvollständige Archiv wurde gelöscht.",
  "progress.okNote": "{files} Dateien und {dirs} Ordner geschrieben — {bytes} auf der Festplatte.",
  "progress.errorNote": {
    one: "{count} Element konnte nicht gelesen werden; ansonsten ist das Archiv vollständig.",
    other: "{count} Elemente konnten nicht gelesen werden; ansonsten ist das Archiv vollständig.",
  },
  "progress.failedNote":
    "Das Archiv konnte nicht fertiggestellt werden. Das Protokoll unten nennt den Grund.",
  "progress.log": "Protokoll",
  "progress.logFailed": "{count} fehlgeschlagen",
  "progress.logEmpty": "Noch nichts protokolliert.",
  "progress.logTrimmed": "die letzten {shown} von {total} werden angezeigt",
  "progress.saveLog": "Protokoll speichern",
  "progress.saveLogTitle": "Protokoll speichern",
  "progress.savedLog": {
    one: "{count} Protokollzeile geschrieben.",
    other: "{count} Protokollzeilen geschrieben.",
  },
  "progress.stopping": "Wird angehalten…",
  "progress.cancel": "Abbrechen",
  "progress.reveal": "Im Ordner anzeigen",
  "progress.done": "Fertig",
  "progress.filterText": "Text",

  // ---------------------------------------------------------------- archive log
  "log.addedFile": "hinzugefügt",
  "log.addedDir": "Ordner hinzugefügt",
  "log.dirFailed": "Ordner konnte nicht hinzugefügt werden: {error}",
  "log.skipped": "übersprungen: {error}",
  "log.padded": "nach Lesefehler mit Füllbytes hinzugefügt: {error}",
  "log.truncated": "nach Lesefehler unvollständig hinzugefügt: {error}",
  "log.createFailed": "Archiv kann nicht erstellt werden: {error}",
  "log.writeFailed": "Schreiben des Archivs fehlgeschlagen: {error}",
  "log.cancelledDeleted": "abgebrochen; das unvollständige Archiv wurde gelöscht",
  "log.cancelledKept":
    "abgebrochen, das unvollständige Archiv konnte aber nicht gelöscht werden: {error}",
  "log.summaryWritten":
    "{files} Dateien und {dirs} Ordner geschrieben, {bytes} Bytes auf der Festplatte",
  "log.summaryErrors": "{errors} nicht lesbar, {skipped} übersprungen",
  "log.summaryElapsed": "abgeschlossen in {seconds} s",
  "log.summaryCancelled": "vom Benutzer abgebrochen",
  "log.summaryFailed": "das Archiv konnte nicht fertiggestellt werden",

  // ---------------------------------------------------------------- settings dialog
  "settings.title": "Einstellungen",
  "settings.appearance": "Darstellung",
  "settings.theme": "Design",
  "settings.language": "Sprache",
  "settings.integration": "Integration",
  "settings.explorer": "In Explorer integrieren",
  "settings.explorerBody":
    "Fügt „{verb}“ zum Kontextmenü von Dateien und Ordnern hinzu. Ein Klick darauf merkt die Auswahl im bereits geöffneten Fenster vor.",
  "settings.explorerNote":
    "Unter Windows 11 erscheint der Eintrag unter „Weitere Optionen anzeigen“.",
  "settings.explorerOn": "Eingetragen",
  "settings.explorerOff": "Nicht eingetragen",
  "settings.explorerVerb": "Mit Tree Archiver archivieren",
  "settings.close": "Schließen",
  "settings.confirmInstallTitle": "Explorer-Eintrag hinzufügen?",
  "settings.confirmInstallBody":
    "Dies schreibt zwei Schlüssel unter HKEY_CURRENT_USER, damit „{verb}“ beim Rechtsklick auf eine Datei oder einen Ordner erscheint. Es betrifft nur Ihr Konto und erfordert keine Administratorrechte.",
  "settings.confirmInstallGo": "Eintrag hinzufügen",
  "settings.confirmRemoveTitle": "Explorer-Eintrag entfernen?",
  "settings.confirmRemoveBody":
    "Dies löscht die beiden Registrierungsschlüssel, die „{verb}“ im Kontextmenü anzeigen. Sonst wird nichts verändert, und Sie können den Eintrag jederzeit wieder hinzufügen.",
  "settings.confirmRemoveGo": "Eintrag entfernen",
  "settings.confirmCancel": "Abbrechen",

  // ---------------------------------------------------------------- dialogs and toasts
  "app.issuesTitle": "Pfade, die nicht gelesen werden konnten",
  "app.issuesLede":
    "Diese wurden beim Einlesen übersprungen. Alles Übrige ist wie gewohnt vorgemerkt.",
  "app.unresolvedTitle": "Einige Planregeln gelten nicht mehr",
  "app.unresolvedLede":
    "Der Baum hat sich seit dem Speichern des Plans geändert. Diese Regeln wurden übersprungen; alles, worauf sie sich bezogen, ist standardmäßig enthalten.",
  "app.planSaved": "Plan gespeichert unter {path}",
  "app.planSaveTitle": "Archivplan speichern",
  "app.planOpenTitle": "Archivplan öffnen",
  "app.planFilter": "Archivplan",
  "app.addFoldersTitle": "Ordner hinzufügen",
  "app.addFilesTitle": "Dateien hinzufügen",
  "app.dismiss": "Schließen",

  // ---------------------------------------------------------------- über
  "about.title": "Über",
  "about.tagline": "Archive großer Verzeichnisbäume planen und erstellen.",
  "about.license": "MIT-LIZENZ",
  "about.licenseTitle": "MIT-Lizenz",
  "about.releaseTitle": "Versionshinweise zu {version}",
  "about.builtLabel": "Build-Datum",
  "about.commitLabel": "Commit",
  "app.close": "Schließen",
};
