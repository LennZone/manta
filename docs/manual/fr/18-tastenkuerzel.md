# Raccourcis clavier {#tastenkuerzel}

Tous les raccourcis de Manta en un coup d’œil, regroupés par thème. La palette de commandes (**⇧⌘P**) affiche le raccourci de chaque commande à côté de celle-ci – celles des plug-ins aussi. La barre des menus liste également chaque commande avec son raccourci (**Fichier**, **Édition**, **Présentation**, **Aller**, **Fenêtre**), comme le Finder – ainsi, les apps qui affichent les raccourcis de l’app active trouvent tous ceux de Manta. Un clic sur une commande de menu l’exécute.

Symboles : **⌘** Commande · **⇧** Maj · **⌥** Option · **⌃** Contrôle · **↩** Retour · **⌫** Suppr. arrière · **⌦** Suppr. avant · **⇥** Tab

## Général {#allgemein}

| Raccourci | Commande |
|---|---|
| ⇧⌘P ou ⌘P | Palette de commandes |
| ⌘, | Réglages… |
| ⌘? | Ouvrir le manuel (ce manuel ; dans la fenêtre d’aide : le champ de recherche) |
| ⌘K | Se connecter au serveur… |
| ⌥⌘L | Afficher les transferts |
| ⇧⌘S | Comparer et synchroniser… |
| ⌘H | Masquer Manta |
| ⌥⌘H | Masquer les autres |
| ⌘Q | Quitter Manta |

## Sous-fenêtres et onglets {#bereiche}

| Raccourci | Commande |
|---|---|
| ⇥ | Changer de sous-fenêtre (vers la suivante, en revenant au début après la dernière) |
| ⌥⌘← / ⌥⌘→ | Activer la sous-fenêtre de gauche / de droite |
| ⌘D | Diviser la sous-fenêtre |
| ⇧⌘D | Fermer la sous-fenêtre |
| ⌘T | Nouvel onglet |
| ⌘W | Fermer l’onglet (dans les Réglages, Comparer et synchroniser et l’aperçu : ferme cette fenêtre) |

## Présentations {#ansichten}

| Raccourci | Commande |
|---|---|
| ⌘1 | Par icônes |
| ⌘2 | Par liste |
| ⌘3 | Par colonnes |
| ⌘4 | Par galerie |
| ⌘ + défilement ou pincement | Modifier la taille des icônes (présentation Icônes) |
| ← / → | Présentation Colonnes : aller à la colonne de gauche / ouvrir le dossier sélectionné dans la colonne suivante |
| ← / → | Présentations Icônes et Galerie : élément précédent / suivant |

Les raccourcis agissent sur la sous-fenêtre active. Voir [Présentations](03-ansichten.md).

## Navigation {#navigation}

| Raccourci | Commande |
|---|---|
| ↩ ou ⌘↓ | Ouvrir (les archives comme des dossiers) |
| ⌥↩ | Ouvrir avec l’app par défaut |
| ⌘↑ ou ⌫ | Dossier parent |
| ⌘[ ou ⌘← | Précédent |
| ⌘] ou ⌘→ | Suivant |
| ⌘L | Modifier le chemin |
| ⇧⌘G | Aller au chemin… |
| ⌘R | Actualiser |
| ⇧⌘. | Afficher/masquer les fichiers masqués |
| ⌘F | Filtrer les fichiers |
| ⌃⌘T | Ajouter le dossier aux Favoris |

## Sélection {#auswahl}

| Raccourci | Effet |
|---|---|
| ↑ / ↓ | Élément précédent / suivant |
| ⇧↑ / ⇧↓ | Étendre la sélection |
| Page précédente / Page suivante | 20 lignes vers le haut / le bas |
| Début / Fin | Premier / dernier élément |
| ⌘A | Tout sélectionner |
| esc | Annuler la sélection (ou fermer l’aperçu) |
| Taper des lettres | Aller au nom correspondant |

## Fichiers {#dateien}

| Raccourci | Commande |
|---|---|
| Espace ou ⌘Y | Coup d’œil / aperçu (Espace ou esc le ferme) |
| F5 | Copier dans l’autre sous-fenêtre… |
| F6 | Déplacer dans l’autre sous-fenêtre… |
| F2 | Renommer… (plusieurs éléments : renommage par lot) |
| ⇧⌘R | Renommer par lot… |
| ⇧⌘N | Nouveau dossier… |
| ⌘⌫ ou ⌦ | Placer dans la corbeille (sur les serveurs : suppression définitive après confirmation, voir [Supprimer](05-dateien.md#loeschen)) |
| ⌥⌘⌫ | Supprimer immédiatement… (définitivement, après confirmation) |
| ⇧⌘⌫ | Vider la corbeille… (après confirmation, voir [Corbeille](07-favoriten-seitenleiste.md#papierkorb)) |
| ⌥⌘C | Compresser… |
| ⌥⌘E | Extraire ici |
| ⌘ pendant le glissement | Présélectionner Déplacer au lieu de Copier (sans touche, Copier est présélectionné ; ⌥ présélectionne explicitement Copier) |

> [!NOTE]
> Sur de nombreux claviers Mac, vous devez maintenir **fn** (🌐) pour **F2**, **F5** et **F6** – sauf si vous avez activé « Utiliser les touches F1, F2, etc. comme des touches de fonction standard » dans Réglages Système → **Clavier**.

## Plug-ins {#plugins}

| Raccourci | Commande | Plug-in |
|---|---|---|
| ⌃` | Afficher/masquer le Terminal | Terminal |
| ⇧⌘B | Afficher/masquer le panneau Git | Git |
| ⌘↩ | Commit (dans le panneau Git) | Git |
| ⌥⌘U | Afficher l’utilisation du disque | Tailles des dossiers |
| ⌥⌘M | Afficher/masquer l’aperçu Markdown | Aperçu Markdown |
| ⌥⌘R | Réexécuter le dernier script | Infos projet |

Vos propres actions rapides peuvent avoir leur propre raccourci (le champ `shortcut` dans `actions.json`).

## Dans les zones de dialogue et les fenêtres {#dialoge}

| Raccourci | Effet |
|---|---|
| ↩ | Confirmer (Copier, Créer, Renommer…) |
| esc | Annuler la zone de dialogue |
| ⌘W | Fermer les Réglages, Comparer et synchroniser ou l’aperçu |

**Palette de commandes** : ↑ ↓ pour choisir · ↩ pour exécuter · esc pour fermer

**Aller au chemin** : ↩ ouvre dans un nouvel onglet · ⌘↩ ouvre dans cet onglet · ⇥ complète · esc annule

## Quand les raccourcis ne fonctionnent pas {#fokus}

- Dans un **champ de texte** (filtre, chemin, champs de zone de dialogue), seuls **⇧⌘P**, **⇧⌘G**, **⌘,** et **⌘?** fonctionnent ; toutes les autres touches s’adressent au champ.
- Dans le **terminal**, **⌘C**, **⌘V**, **⌘A**, **⌘K**, **⌘←**, **⌘→** et **⌘⌫** s’adressent au terminal. Cliquez dans la liste des fichiers pour rendre le clavier à Manta.
- Dans les fenêtres **Réglages**, **Comparer et synchroniser** et **aperçu**, seuls leurs propres raccourcis fonctionnent.

## Différences avec le Finder {#finder}

| Raccourci | Finder | Manta |
|---|---|---|
| ⌘D | Dupliquer | Diviser la sous-fenêtre |
| ⇧⌘D | Ouvrir le Bureau | Fermer la sous-fenêtre |
| ⇧⌘P | Afficher l’aperçu | Palette de commandes |
| ⌘R | Afficher l’original | Actualiser |
| ⌥⌘C | Copier le chemin d’accès | Compresser… |
| ⌥⌘L | Ouvrir Téléchargements | Afficher les transferts |
| ⌫ | – | Dossier parent |

Comme dans le Finder : **⌘⌫** place dans la corbeille, **⌥⌘⌫** supprime immédiatement, **⇧⌘⌫** vide la corbeille, **⌘T** ouvre un nouvel onglet, **⌘Y** ouvre Coup d’œil, **⇧⌘N**, **⇧⌘G**, **⇧⌘.**, **⌘[** / **⌘]**, **⌘↑** / **⌘↓**, **⌘1** – **⌘4**, **⌘K** et **⌃⌘T** (Ajouter à la barre latérale/aux Favoris). Le glissement présélectionne Copier, même sur le même volume (où le Finder déplace) ; **⌘** présélectionne Déplacer.

> [!NOTE]
> **Modifié dans cette version :** **⌘T** ouvre un nouvel onglet (auparavant : **⇧⌘T**), les transferts sont passés à **⌥⌘L** (auparavant : **⌘T**), **⌘⌫** place les éléments locaux dans la corbeille (auparavant : suppression définitive ; c’est désormais **⌥⌘⌫**). Manta l’indique une fois après la mise à jour.
