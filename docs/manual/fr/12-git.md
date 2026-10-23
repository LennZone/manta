# Git {#git}

Lorsqu’un dossier se trouve dans un dépôt Git, Manta affiche l’état de chaque fichier directement dans la liste, la branche dans la barre du chemin d’accès, ainsi qu’un panneau Git pour indexer et créer des commits. `git` doit être installé sur votre Mac (par exemple avec les Xcode Command Line Tools).

![Liste des fichiers avec des pastilles Git et le panneau Git](images/git-panel.png)

## État dans la liste des fichiers {#abzeichen}

Les fichiers modifiés portent une pastille. Survolez-la : une infobulle explique l’état.

- **Modifié, non indexé** / **Indexé pour le commit** / **Modifié, partiellement indexé**
- **Ajouté, indexé pour le commit**
- **Non suivi** (nouveau, Git ne connaît pas encore le fichier)
- **Supprimé, non indexé** / **Suppression indexée pour le commit**
- **Renommé depuis…** / **Copié depuis…**
- **Conflit de fusion**
- **Ignoré par .gitignore** – ces fichiers sont estompés.

Les dossiers indiquent combien de fichiers modifiés ils contiennent (« 3 fichiers modifiés à l’intérieur »).

## Branche dans la barre du chemin d’accès {#chip}

Une pastille dans la barre du chemin d’accès indique la branche actuelle. Son infobulle vous en dit plus :

- la branche amont suivie et si vous êtes en avance ou en retard (« Suit origin/main (2 en avance) »),
- le dernier commit,
- « HEAD détachée sur… » lorsqu’aucune branche n’est extraite.

Si une fusion, un rebasage, un cherry-pick, un revert ou une bissection est en cours, la pastille l’indique. Un clic sur la pastille ouvre le panneau Git.

## Le panneau Git {#seitenleiste}

Pour l’afficher ou le masquer : **⇧⌘B** (**Afficher/masquer le panneau Git**) ou cliquez sur la pastille de la branche.

Le panneau affiche :

- en haut, la branche, la branche amont et le nombre de commits à pousser ou à tirer,
- les groupes **Conflits**, **Indexé**, **Modifications** et **Non suivi**,
- un champ pour le **Message de commit**,
- les **Commits récents**.

### Créer un commit {#commit}

1. Ouvrez le panneau Git avec **⇧⌘B**.
2. Indexez des fichiers : **Indexer** sur un fichier, ou **Tout indexer** sur le groupe **Modifications**.
3. Cliquez sur un fichier pour voir son diff.
4. Tapez un **Message de commit**.
5. Cliquez sur **Commit** ou appuyez sur **⌘↩**.

Manta indique « Commit abc1234 créé : … ».

Pour annuler l’indexation : **Désindexer** ou **Tout désindexer**.

### Résoudre les conflits {#konflikte}

Lors d’un conflit de fusion, les fichiers concernés sont listés sous **Conflits**. Résolvez le conflit dans votre éditeur, puis choisissez **Marquer comme résolu (indexer)**. Une fois tous les conflits résolus, le panneau indique « Créez le commit pour terminer la fusion ». Pour un rebasage, un cherry-pick ou un revert, il affiche la commande `--continue` correspondante à saisir dans le terminal.

## Actualiser {#aktualisieren}

Manta actualise automatiquement l’état Git. Si nécessaire : **Actualiser l’état Git** dans la palette de commandes, ou le bouton d’actualisation du panneau.

## Limites {#grenzen}

- Git ne fonctionne que pour les dossiers **locaux** (« L’état Git est disponible pour les dossiers de ce Mac, pas sur les serveurs. »).
- Pousser, tirer, changer de branche et fusionner se font dans le [Terminal](11-terminal.md) ou dans une app Git. Le panneau affiche l’état, indexe et crée des commits.
