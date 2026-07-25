# Couverture à 100 % : diagnostiquer par instanciation, combler le côté le moins cher

## Contexte

`CLAUDE.md` exige que `make test` atteigne 100 % de couverture une fois une fonctionnalité terminée.
À la fin de la phase 3, `index.rs` plafonnait à 89,81 % de *régions* alors que toutes les lignes de logique semblaient exercées.
Les régions manquantes étaient des arêtes d'erreur de `?` : chaque `?` compile vers une branche, donc `connection.execute_batch(SCHEMA)?` compte deux régions — succès et échec.

Une fois ces arêtes couvertes, le compteur est resté bloqué à 95,67 % alors que la vue fusionnée (`segments`) ne montrait **aucune** position non couverte.
Cette contradiction a d'abord été prise pour un artefact insurmontable de l'outil ; c'est faux, et c'était exactement le piège documenté ailleurs (voir *Antécédent*).

## Décision

**`make test` échoue sous 100 % de régions, de lignes et de fonctions.**

```
cargo +nightly llvm-cov --ignore-filename-regex '(lib\.rs|/mod\.rs|/main\.rs)$' \
    --fail-under-regions 100 --fail-under-lines 100 --fail-under-functions 100
```

### La règle opérationnelle

`llvm-cov` replie un groupe d'instanciations en prenant le **maximum du nombre de régions couvertes d'une seule copie**, jamais l'union des copies.
Un fichier à tests unitaires en ligne est compilé deux fois — une fois nu (lié au binaire d'intégration), une fois avec `#[cfg(test)]` (binaire de tests unitaires) — donc :

> **Le 100 % exige qu'une seule compilation couvre tout le fichier.**

Toute vue de type union — `segments`, lcov, HTML — affichera 100 % pendant que le résumé annonce un manque.
**Cette contradiction est la signature du repliage, pas la preuve d'un fantôme.**

### La procédure de diagnostic

Une couverture de régions sous 100 % sur un fichier à tests en ligne se ventile **avant** d'écrire quoi que ce soit :

1. `cargo +nightly llvm-cov --json` ;
2. grouper les entrées de `functions` par empreinte de span (première région + nombre de régions) ;
3. par groupe, calculer le nombre de régions couvertes de **chaque copie** ; le groupe fautif est celui où `max(couvertes) < total` ;
4. lister les régions manquées par chaque copie, puis combler le côté **mesuré** le moins cher.

Écrire un test sans cette ventilation revient à deviner de quel côté est le trou.
Ici, la ventilation a donné en une passe : `open` 20/22, `rebuild` 17/19, `scan_vault` 45/47, `insert_note` 58/61, `discard` 9/11, `query_rows` 17/18 — et pour chacun le côté le moins cher était le binaire de tests unitaires, à 1 à 3 régions près, contre 2 à 27 côté intégration.

### Le piège des fonctions génériques

`query_rows` est générique sur le type des paramètres.
Un test qui appelait `query_paths(conn, sql, [])` a donc créé une **instanciation supplémentaire** (6/18) au lieu de couvrir celle des méthodes publiques, qui passent `[&str; 1]`.
Le type des arguments fait partie de l'identité de la copie : un test de couverture visant une fonction générique doit passer **les mêmes types** que le code de production, sinon il couvre une copie à lui.

### Ce qui a servi à couvrir les arêtes d'erreur

Trois moyens, du moins cher au plus cher :

1. **États réels du système de fichiers et de SQLite** — un chemin sous un répertoire inexistant fait échouer `Connection::open` ; un *fichier* nommé `permanent` fait échouer le `read_dir` interne ; un blob planté par une connexion brute casse `row.get::<String>` (l'affinité TEXT convertit les nombres, jamais les blobs) ; un `DROP TABLE` ne fait échouer `prepare` qu'au **deuxième** appel, SQLite gardant le schéma en cache par connexion et échouant à `step` la première fois.
2. **L'autorisateur de SQLite** — `Connection::authorizer` consulte un callback à la préparation de chaque instruction ; `Authorization::Deny` transforme l'instruction choisie en erreur. « Faire échouer l'INSERT dans `tags` mais pas celui dans `notes` » tient en quatre lignes, sans trait ni faux objet. Impose la feature `hooks` de rusqlite, déclarée dans `[dependencies]` faute de features par profil.
3. **Un module `faults` compilé sous `cfg(test)`** — hors test, chacune de ses fonctions est l'identité. Il couvre les quatre arêtes qu'aucun état réel n'atteint sous Linux : seconde ouverture de connexion dans `open`, échec du `PRAGMA foreign_keys`, échec de la création du schéma, itérateur `read_dir` rendant une `Err` en cours de route. Chaque point d'injection renvoie une **valeur** que le vrai code utilise ensuite (`execute_batch(faults::schema_sql())`), jamais un `?` supplémentaire : le flot de contrôle testé est le flot de contrôle livré.

Les tests d'injection vivent dans `src/index.rs` : forcer une panne demande la connexion privée, et le module de tests unitaires est la frontière de privilège qui l'autorise sans élargir l'API publique.

### Deux changements de production sortis de cette chasse

Ils se justifient seuls, indépendamment du compteur :

- `discard()` remplace `if db_path.exists() { remove_file(..)? }` par un `match` traitant `NotFound` comme un succès — le pré-test laissait une fenêtre TOCTOU où un autre processus supprimait le fichier et où l'on rapportait son absence comme un échec.
- `query_rows()` fusionne les corps quasi identiques de `query_paths` et `dangling_links`, qui préparaient, mappaient et collectaient chacun de leur côté.

## Alternatives rejetées

- **Baisser le seuil aux lignes seules** (première rédaction de cet ADR, erronée) : justifiée par un prétendu artefact insurmontable, alors que le repliage était simplement mal compris — la somme par fichier avait été prise pour une somme de copies au lieu d'un maximum par groupe. Un seuil baissé aurait figé l'erreur dans l'outillage.
- **Un seuil arbitraire (`--fail-under-regions 95`)** : nombre magique injustifiable six mois plus tard, qui pourrit dès qu'un fichier bouge.
- **`coverage(off)` sur les fonctions récalcitrantes** : 100 % par non-mesure.
- **Injecter le système de fichiers et rusqlite derrière des traits** : change les signatures de `Index::open` et `scan_vault` de façon permanente et impose des faux objets fidèles aux sémantiques d'échec réelles de SQLite — lesquelles ne sont pas évidentes, comme l'a montré le `DROP TABLE` qui échoue à `step` et non à `prepare`.
- **`--fail-under-file-lines 100`** : échoue à 100,00 % alors qu'il passe à 99,9 %, visiblement une comparaison de bord dans l'outil. `--fail-under-lines 100` sur le total est aussi strict : au seuil de 100 %, une seule ligne non couverte où que ce soit fait tomber le total.

## Si 100 % devient réellement impossible plus tard

Ce sera le cas si une ligne de production n'est atteignable ni par un état réel, ni par l'autorisateur, ni par une faute `cfg(test)`, **et** que la ventilation par instanciation confirme qu'aucune copie ne peut la couvrir.
Dans ce cas : **ne pas baisser le seuil**, ajouter `#[cfg_attr(coverage_nightly, coverage(off))]` sur la fonction concernée avec un commentaire nommant la raison exacte.
Les exemptions restent visibles et révisables une par une au lieu de se cacher dans un pourcentage.

Condition préalable obligatoire : avoir fait la ventilation des étapes 1 à 4 ci-dessus.
Sans elle, « impossible » signifie seulement « pas encore diagnostiqué » — c'est précisément l'erreur commise lors de la rédaction initiale de cet ADR.

## Antécédent

Le même phénomène avait déjà été rencontré et documenté dans `generateur_horaire` :
`docs/conception/adr/2026-07-couverture-par-instanciation-le-plus-petit-ecart.md` (2026-07-19) et
`docs/conception/adr/2026-07-couverture-100-et-frontiere-io.md` (2026-07-17).
