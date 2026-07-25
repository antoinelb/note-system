# Surveillance du vault : mise à jour incrémentale, repli sur reconstruction

## Contexte

L'index doit rester à jour pendant que l'on écrit, sans que l'on ait à le reconstruire à la main.
`roadmap-v0.md` prévoyait « débruiter les rafales ; sur tout ce qui est ambigu, retomber sur une reconstruction complète ».
Deux contraintes de disposition pèsent sur la conception :

- `.index/` vit **à l'intérieur** du vault, donc une surveillance récursive de la racine voit les écritures de SQLite ;
- `templates/` vit au même niveau que les catégories mais n'est pas indexé.

## Décision

### Débruitage : `notify-debouncer-full`

Un seul `:w` produit plusieurs événements inotify ; sans débruitage la même note est analysée trois fois par sauvegarde.
La fenêtre de silence est de 200 ms.

Le gestionnaire passé au débruiteur est une **fermeture**, pas un `Sender` — `impl<F> DebounceEventHandler for F where F: FnMut(DebounceEventResult) + Send + 'static` (`notify-debouncer-full` lib.rs:120-127).
Le débruiteur possède déjà son fil d'exécution et y appelle le gestionnaire, donc la classification se fait dedans et nous n'avons aucun fil à nous.
C'est ce qui évite d'écrire ici la boucle d'événements sans condition de terminaison que les règles de codage interdisent : elle reste dans la bibliothèque.

La caisse réexporte `notify` (lib.rs:86), donc aucune seconde dépendance n'est déclarée.

### Mise à jour : incrémentale, avec repli

Une note touchée est remplacée seule (`Index::update_note`), une note supprimée est retirée seule (`Index::remove_note`).
Le coût de ce choix est un **second chemin d'écriture** dans l'index, qui pourrait diverger de `rebuild`.
Il est neutralisé par la structure, pas par la discipline :

- `update_note` supprime puis appelle le **même** `insert_note` que `rebuild` — les deux chemins ne peuvent pas diverger sur la façon dont une note est stockée, seulement sur *quelles* notes le sont ;
- le `DELETE` sur `notes` fait tomber les lignes de `tags`, `links` et `anomalies` par `ON DELETE CASCADE`, donc aucune ligne fille ne peut survivre à un remplacement.

Les liens pendants et les notes sans type restent corrects sans traitement particulier : ce sont des requêtes sur la jointure complète, calculées à la lecture, jamais matérialisées.
Créer la note cible d'un lien pendant le fait donc cesser d'être pendant sans qu'aucune ligne de `links` ne soit touchée.

`VaultChange::Rescan` déclenche une reconstruction complète et **termine le lot** : une fois qu'on sait que des événements ont été perdus, les mises à jour suivantes s'appliqueraient sur un état auquel on ne peut plus se fier.
Trois choses produisent un `Rescan` : un lot en erreur, `Event::need_rescan()`, et tout événement qu'on ne sait pas attribuer (`EventKind::Any`, `Other`, `RenameMode::Any`, `RenameMode::Other`).

### Remise au consommateur : un `Receiver<Vec<VaultChange>>`

Le guetteur n'écrit jamais dans l'index ; il rapporte ce qui a changé et l'appelant décide quand l'appliquer.
L'index garde donc un seul propriétaire, sans `Mutex`, et la phase 4 branchera le canal sur une tâche Dioxus.
Les tests, eux, utilisent `recv_timeout` : aucune boucle d'attente, aucun `sleep`.

Un lot vide n'est **pas** envoyé.
Sans cette règle, chaque écriture de SQLite réveillerait l'appelant avec zéro travail à faire.

### Le filtre n'est pas une liste d'exclusions

`note_path` accepte un fichier `.typ` situé directement sous un répertoire que `NoteCategory::from_dir` reconnaît.
`.index/`, `templates/`, les sous-répertoires et les fichiers non `.typ` sont donc exclus **parce qu'ils ne sont pas des catégories**, pas parce qu'ils sont nommés quelque part.
C'est exactement l'autorité qu'utilise déjà `scan_vault` : il ne peut pas y avoir de désaccord entre ce que la reconstruction indexe et ce que le guetteur surveille.

Ce point est ce qui empêche la boucle de rétroaction : une reconstruction écrit dans `.index/`, le guetteur le voit, la classification rend une liste vide, rien n'est envoyé.
Le filtre de chemin passe **avant** l'analyse du genre d'événement, sinon un `EventKind::Any` sur `index.db-wal` s'escaladerait en `Rescan`, donc en reconstruction, donc en nouvelle écriture dans `.index/`.

### Renommages

`notify-debouncer-full` corrèle un renommage en un seul événement `Modify(Name(RenameMode::Both))` dont les chemins sont `[origine, destination]` (lib.rs:455-462).
Ils sont donc lus **par position** : le premier est retiré, le second est analysé.
Chaque moitié peut tomber hors du vault et disparaître seule — déplacer une note hors des catégories est une suppression, l'y déplacer est une création.

### Couverture : une injection, deux branches mortes supprimées

`new_debouncer` ne peut échouer, sur un système sain, que par épuisement des descripteurs de fichiers.
Il valide toutefois ses arguments avant de créer quoi que ce soit : un `tick_rate` supérieur au délai de débruitage est refusé avant le lancement du fil et la création du guetteur (lib.rs:644-651).
`faults::tick_rate()` exploite ce point — `None` hors test, `QUIET * 2` une fois armé — sur le modèle de `index::faults` : l'injection rend une **valeur** que le vrai code utilise, jamais un `?` supplémentaire.
Comme le module `faults` est en `cfg(test)`, seule la compilation des tests unitaires peut atteindre cette arête ; c'est pourquoi `start` est testé de bout en bout dans `src/watch.rs` et non seulement depuis `tests/integration/`.

La ventilation par instanciation a aussi révélé deux régions qu'aucun test ne pouvait atteindre, corrigées en **production** plutôt que contournées :

- `relative.parent()?` dans `note_path` — `Path::parent()` ne rend `None` que pour `""` et `/`, or le test d'extension situé au-dessus rejette déjà ces deux cas. Remplacé par `unwrap_or(Path::new(""))`, que `from_dir` refuse de toute façon.
- le bras `(None, None)` du renommage — la garde `note_paths.is_empty()` garantit qu'au moins un chemin de l'événement est une note, et un renommage corrélé en porte exactement deux ; les deux moitiés ne peuvent donc pas être toutes deux hors vault. Les quatre bras sont remplacés par `first.into_iter().chain(second).collect()`.

Règle qui s'en dégage : une région inatteignable signale presque toujours qu'une ligne antérieure a déjà tranché la question.
Le seuil ne demandait pas un test de plus, il désignait une branche qui n'en était pas une.

## Alternatives rejetées

- **Reconstruction complète à chaque rafale** : c'était la recommandation initiale et le pré-engagement de la feuille de route. Rejetée par choix explicite : à l'échelle d'un vault personnel la reconstruction reste peu chère, mais elle réécrit toutes les lignes à chaque frappe débruitée, et le repli sur reconstruction est de toute façon conservé pour les cas ambigus, donc le chemin incrémental n'ajoute pas de mode de défaillance nouveau — seulement un chemin rapide.
- **Un `notify::RecommendedWatcher` nu avec minuterie maison** : un fil, une minuterie et un tampon partagé à écrire et à couvrir à 100 %, contre une dépendance qui fait déjà la corrélation des renommages par identifiant de fichier.
- **Exclure `.index/` par un test de chemin explicite** : deux définitions concurrentes de « ceci est une note », qui divergeraient le jour où une catégorie est ajoutée.
- **Le guetteur possède l'`Index`** : impose un `Arc<Mutex<Index>>` et fait passer chaque requête de l'interface par un verrou, pour éviter une ligne de branchement dans la phase 4.
- **Un rappel (`callback`) plutôt qu'un canal** : le rappel s'exécute sur un fil que nous ne contrôlons pas, et les tests devraient de toute façon en extraire l'état par un `Arc<Mutex<_>>`.

## Conséquence

`docs/plan.md` n'a pas besoin d'être modifié : « l'index est reconstruit en analysant les fichiers ; un guetteur le tient à jour » reste exact, et la règle « sur tout ce qui est ambigu, reconstruction complète » est appliquée telle quelle.

En phase 4, l'interface consommera `VaultWatcher::changes` depuis une tâche Dioxus et appellera `watch::apply`.
