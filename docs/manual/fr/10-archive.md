# Archives {#archive}

Manta ouvre les archives ZIP et TAR comme des dossiers : vous voyez immédiatement le contenu, sans extraction préalable. Ce chapitre explique aussi comment extraire des archives et en créer de nouvelles.

## Formats pris en charge {#formate}

- `.zip`
- `.tar`, `.tar.gz` / `.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`
- fichiers compressés individuels : `.gz`, `.bz2`, `.xz`, `.zst`

## Ouvrir une archive comme un dossier {#oeffnen}

1. Sélectionnez l’archive.
2. Appuyez sur **↩** ou double-cliquez dessus.

La barre du chemin d’accès affiche l’archive comme un dossier, par exemple `Téléchargements › backup.zip › docs`. Une pastille avec un cadenas indique que la sous-fenêtre affiche une **archive en lecture seule**.

- **⌫** ou **⌘↑** dans le dossier de plus haut niveau de l’archive ramène au dossier qui contient l’archive, celle-ci étant sélectionnée.
- **Précédent** et **Suivant** fonctionnent à travers l’archive.

![Une archive ZIP ouverte comme un dossier](images/archive-as-folder.png)

Autres possibilités :

- **⌥↩** (**Ouvrir avec l’app par défaut**) ouvre l’archive avec son app par défaut (l’Utilitaire d’archive de macOS, par exemple).
- Les documents qui sont en réalité des fichiers ZIP (`.docx`, `.xlsx`, `.jar`, `.apk`…) s’ouvrent toujours dans leur app. Clic droit → **Parcourir l’archive** affiche leur contenu.
- Si vous ne voulez jamais que les archives s’ouvrent comme des dossiers, désactivez **Réglages** → **Général** → **Ouvrir les archives comme des dossiers**.

Les archives situées sur des serveurs et les archives contenues dans des archives sont d’abord téléchargées dans un fichier temporaire (avec la progression et **Annuler** dans la sous-fenêtre). Les grandes archives sont lues en arrière-plan.

## Ce qui fonctionne dans une archive – et ce qui ne fonctionne pas {#schreibgeschuetzt}

Les archives sont en **lecture seule**. Manta refuse, avec une indication, la création de dossiers, le renommage, la suppression, le déplacement et le dépôt de fichiers dans l’archive.

La **copie vers l’extérieur** fonctionne toujours :

- avec **F5** vers l’autre sous-fenêtre,
- en faisant glisser vers une autre sous-fenêtre,
- avec **Extraire ici** ou **Extraire dans l’autre sous-fenêtre**.

## Extraire une archive {#entpacken}

**Une archive entière :**

1. Sélectionnez le fichier d’archive.
2. Appuyez sur **⌥⌘E** (**Extraire ici**) – ou clic droit → **Extraire ici** ou **Extraire dans l’autre sous-fenêtre**.

**Extraire ici** décompresse à côté de l’archive : dans un dossier portant le nom de l’archive, ou directement si l’archive ne contient qu’un seul élément.

**Éléments individuels d’une archive ouverte :**

1. Ouvrez l’archive comme un dossier et sélectionnez les éléments.
2. **⌥⌘E** les extrait dans le dossier qui contient l’archive. **F5** ou clic droit → **Extraire dans l’autre sous-fenêtre…** les extrait dans l’autre sous-fenêtre.

L’extraction s’exécute comme un transfert en arrière-plan (voir [Transferts](05-dateien.md#uebertragungen)).

## Créer une archive {#komprimieren}

1. Sélectionnez des fichiers et des dossiers.
2. Appuyez sur **⌥⌘C** (**Compresser…**) ou choisissez cette commande dans le menu contextuel.
3. Choisissez le **Format** : zip, tar.gz, tar.xz ou tar.zst.
4. Vérifiez le **Nom**. Manta en propose un, comme « site.zip » ou « Archive 2.zip ».
5. Utilisez le curseur **Niveau** pour choisir entre **Plus rapide** et **Plus petit**.
6. **Exclure .DS_Store et __MACOSX** est activé : aucun fichier auxiliaire du Mac ne se retrouve ainsi dans l’archive.
7. Cliquez sur **Compresser**.

L’archive est créée dans le dossier actuel. Cela fonctionne aussi sur les serveurs : Manta construit l’archive dans un fichier temporaire local, puis la téléverse.

![Feuille Compresser](images/compress-sheet.png)

> [!TIP]
> Pour les utilisateurs de Mac, **zip** est le plus pratique. **tar.zst** est très rapide et compact, mais nécessite un outil adapté chez le destinataire.

## Sécurité {#sicherheit}

- Les entrées contenant `..` ou des chemins absolus pourraient, lors de l’extraction, écrire en dehors du dossier cible. Manta ne les affiche ni ne les extrait jamais et signale « N entrées non sûres ignorées ».
- Les entrées ZIP chiffrées portent une pastille avec un cadenas. Manta ne peut ni les ouvrir ni les extraire et ne demande pas de mot de passe.

## Limites {#grenzen}

- Aucune modification à l’intérieur des archives (ajout, suppression, renommage).
- Pas d’entrées ZIP protégées par mot de passe.
- Les liens symboliques sont affichés en tant que liens ; l’extraction copie le fichier cible, et les liens vers des dossiers sont ignorés.
- Les TAR compressés de plus de 2 Go deviennent plus lents, car Manta les relit depuis le début pour chaque fichier.
- Les archives ZIP fractionnées et les fichiers TAR clairsemés (sparse) ne sont pas pris en charge.
