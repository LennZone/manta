# Plug-ins intégrés {#plugins}

De nombreuses fonctions de Manta sont fournies par des plug-ins intégrés. Ils sont activés dès le départ, et vous pouvez désactiver chacun d’eux dans **Réglages** → **Plug-ins**. Git et Terminal ont leur propre chapitre : [Git](12-git.md), [Terminal](11-terminal.md).

## Activer et désactiver des plug-ins {#verwalten}

1. Ouvrez **Réglages** avec **⌘,** et choisissez **Plug-ins**.
2. Les plug-ins intégrés se trouvent sous **Intégrés**. Cliquez sur **Désactiver** ou **Activer**.
3. Si un plug-in a des options, ouvrez-les avec **Réglages** sur sa carte.

Pour ajouter des plug-ins d’autres personnes, ouvrez **Découvrir** en haut de la page Plug-ins, double-cliquez sur un fichier `.mantaplugin`, déposez-le sur la page Plug-ins ou utilisez **Installer un plug-in…** – à partir d’un fichier `.mantaplugin` ou d’un dossier. Les plug-ins d’autres personnes s’exécutent dans un bac à sable macOS et ne peuvent faire que ce que leurs autorisations permettent ; les plug-ins intégrés font partie de Manta et ne sont pas isolés. Pour en savoir plus : [Vos propres plug-ins](15-eigene-plugins.md#installieren) et [Sécurité](15-eigene-plugins.md#sicherheit).

De nombreux plug-ins affichent leur contenu dans un **panneau latéral à droite** ou dans un **panneau sous la sous-fenêtre**. Ouvrez-les avec des raccourcis, des pastilles de la barre du chemin d’accès ou la palette de commandes (« Afficher le panneau « … » »).

![Réglages → Plug-ins](images/settings-plugins.png)

## Tailles des dossiers {#ordnergroessen}

**Tailles des dossiers** affiche aussi la taille réelle des dossiers dans la colonne **Taille** – mesurée en arrière-plan et mise en cache. Les tailles sont conservées après un redémarrage : Manta les affiche aussitôt et les remesure en arrière-plan au cas où quelque chose aurait changé. Lorsque quelque chose change dans un sous-dossier, même depuis une autre app, Manta le remesure peu après de lui-même.

- Tant que Manta compte encore, il affiche par exemple « ≥ 1,2 Go » avec un petit indicateur d’activité.
- Survolez une taille pour voir le nombre de fichiers et la taille sur le disque.
- Vous pouvez trier par taille de dossier, et celle-ci est prise en compte dans la taille de la sélection.

La taille est la somme des tailles des fichiers (comme pour les fichiers). Manta ne suit pas les liens symboliques, ne compte les liens physiques qu’une fois et reste sur le volume du dossier.

**Uniquement sur demande** : si vous réglez **Réglages** → **Plug-ins** → **Tailles des dossiers** sur **Uniquement sur demande**, les dossiers affichent « — » jusqu’à ce que vous fassiez un clic droit dessus et choisissiez **Calculer la taille**.

### Utilisation du disque {#speicherbelegung}

**⌥⌘U** (**Afficher l’utilisation du disque**) montre ce qui occupe de l’espace dans le dossier actif :

- une **treemap** : cliquez pour zoomer, double-cliquez pour ouvrir,
- une liste des **Éléments les plus volumineux**.

![Utilisation du disque avec une treemap](images/disk-usage.png)

Les tailles des dossiers et l’utilisation du disque ne concernent que les dossiers locaux.

## Renommage par lot {#umbenennen}

Renommez de nombreux fichiers à la fois – avec des règles, un aperçu en direct et une fonction d’annulation.

1. Sélectionnez les fichiers. Si rien n’est sélectionné, l’opération s’applique à tous les fichiers du dossier.
2. Appuyez sur **⇧⌘R** (**Renommer par lot…**) ou sur **F2** lorsque plusieurs éléments sont sélectionnés.
3. Ajoutez des règles avec **Ajouter une règle**. Elles s’appliquent de haut en bas.
4. Vérifiez l’**Aperçu** : nom d’origine → nouveau nom, avec les modifications mises en évidence caractère par caractère.
5. Cliquez sur **Renommer** (ou appuyez sur ↩).

![Renommage par lot avec des règles et l’aperçu](images/batch-rename.png)

Les règles :

| Règle | Effet |
|---|---|
| **Rechercher et remplacer** | Texte ou expression régulière, avec des groupes `$1` ; respect de la casse ; première occurrence ou toutes. |
| **Insérer du texte** | Au début, à la fin ou à une position donnée. |
| **Supprimer des caractères** | Les premiers, les derniers ou une plage de caractères. |
| **Modifier la casse** | minuscules, MAJUSCULES, Titre, Phrase. |
| **Numérotation** | 1, 2, 3… avec début, pas, chiffres (remplissage), en préfixe, en suffixe ou comme nom. |
| **Date** | Date de modification ou date du jour, dans le format de votre choix. |
| **Extension** | Modifier l’extension, la passer en minuscules/majuscules ou la supprimer. |
| **Nettoyer** | Supprimer les espaces aux extrémités, fusionner les espaces, translittérer les accents (ä → ae, é → e). |

Chaque règle peut s’appliquer au **Nom**, à l’**Extension** ou aux **Deux**, être désactivée et réactivée, et être déplacée.

Sécurité :

- L’aperçu signale immédiatement les conflits (noms en double, noms existants, noms vides ou non valides). Le renommage n’est possible qu’une fois tous les conflits résolus.
- Rien n’est jamais écrasé. L’échange de noms (a ↔ b) et les changements de casse seuls fonctionnent.
- Ensuite, un message propose **Annuler**. La commande se trouve aussi dans la palette de commandes : **Annuler le renommage par lot**.

**Préréglages** : sous **Préréglages**, vous enregistrez un ensemble de règles sous un nom et le rechargez plus tard.

Le renommage par lot fonctionne sur votre Mac et sur les serveurs.

## Actions rapides {#schnellaktionen}

Les **Actions rapides** sont vos propres commandes pour les fichiers sélectionnés : redimensionner des images, compresser en ZIP, ouvrir dans VS Code, calculer des sommes de contrôle et tout ce qu’une commande shell peut faire.

### Exécuter une action rapide

- **Clic droit** sur des fichiers → sous-menu **Actions rapides**. Il ne liste que les actions adaptées à la sélection (« Réduire les images à 50 % » uniquement pour les images, par exemple).
- **Palette de commandes** (**⇧⌘P**) : tapez le nom de l’action.
- **Panneau** : **Afficher le panneau Actions rapides** dans la palette affiche les actions applicables et, sous **Dernières exécutions**, l’état, la durée et la sortie.

Deux actions en arrière-plan au maximum s’exécutent en même temps. Si l’une dure plus longtemps, un message avec **Annuler** apparaît ; à la fin, vous recevez un message avec le résultat. Les actions de terminal s’exécutent de façon visible dans le [Terminal](11-terminal.md) sous la sous-fenêtre.

### Actions fournies avec Manta

Réduire les images à 50 % · Convertir en JPEG · Convertir en PNG · Compresser en ZIP · Extraire l’archive · Ouvrir dans VS Code · Ouvrir dans Terminal.app · Copier le chemin · Afficher dans le Finder · Rendre exécutable · Compter les lignes · Calculer le SHA-256 · Copier dans l’autre sous-fenêtre (rsync)

### Vos propres actions dans actions.json {#actions-json}

Les actions se trouvent dans `actions.json`, dans le dossier de données du plug-in (`~/Library/Application Support/Manta/plugin-data/manta.quickactions/`).

1. Choisissez **Modifier les actions rapides…** dans la palette de commandes. Le fichier s’ouvre dans votre éditeur de texte.
2. Modifiez ou ajoutez des actions, puis enregistrez le fichier.
3. Manta le recharge en moins d’une seconde et signale les problèmes (« Actions rapides : … »). Manuellement : **Recharger les actions rapides**.

Un exemple :

```jsonc
{
  "actions": [
    {
      "id": "optimize",
      "title": "Optimize PNG",
      "title_de": "PNG optimieren",
      "applies_to": ["png"],
      "command": "optipng {file}",
      "message": "Optimized {count} image{s}",
      "message_de": "{count} Bild(er) optimiert",
      "refresh": true
    }
  ]
}
```

Les champs les plus importants :

| Champ | Signification |
|---|---|
| `id` | Identifiant unique (lettres, chiffres, `.`, `-`, `_`). |
| `title`, `title_de` | Nom dans le menu ; `title_de` est utilisé lorsque Manta fonctionne en allemand. |
| `command` | Commande shell, exécutée avec `/bin/sh -c` dans le dossier de la sélection. |
| `applies_to` | `"any"`, `"files"`, `"folders"` et/ou des extensions comme `"jpg"`. |
| `min`, `max` | Nombre d’éléments devant être sélectionnés. |
| `mode` | `"background"` (par défaut), `"terminal"` (visible dans le terminal) ou `"silent"`. |
| `output` | `"none"`, `"toast"` (sortie sous forme de message) ou `"panel"`. |
| `confirm` | Question avant l’exécution, par exemple `"Delete {count} file{s}?"`. |
| `refresh` | Relire la sous-fenêtre ensuite. |
| `shortcut` | Un raccourci propre, par exemple `"⌃⌥Z"`. |

Paramètres fictifs dans la commande : `{file}` (la commande s’exécute alors une fois par élément), `{files}` (tous à la fois), `{name}`, `{stem}`, `{ext}`, `{dir}`, `{dest}` (le dossier de l’autre sous-fenêtre) et `{count}`. Manta se charge des guillemets – écrivez `{file}`, et non `'{file}'`.

La référence complète se trouve dans [docs/quick-actions.md](../../quick-actions.md).

> [!TIP]
> Pour obtenir les actions par défaut actuelles après une mise à jour, renommez `actions.json`. Manta recrée le fichier.

Les actions rapides ne fonctionnent que dans les dossiers locaux.

## Infos projet {#projektinfo}

**Infos projet** détecte le projet dans lequel vous vous trouvez et rend ses scripts exécutables en un clic.

Il détecte : Laravel (`artisan`), Composer, Node (npm, pnpm, yarn, bun), Docker Compose, Rust (Cargo), Python, Make, just et Task.

- **Pastille dans la barre du chemin d’accès** : par exemple « Laravel · 28 scripts ». Cliquez pour afficher ou masquer le panneau Projet.
- **Panneau Projet** : nom, racine, branche Git et versions (« PHP ^8.2 », par exemple), les boutons **Ouvrir dans VS Code**, **Finder**, **.env** et **Aller à la racine**, vos scripts récents et tous les scripts regroupés par outil.
- **Un clic sur un script** l’exécute dans le [Terminal](11-terminal.md) sous la sous-fenêtre, à la racine du projet – comme si vous l’aviez tapé.
- **Palette de commandes** : **Exécuter un script du projet…**, **Afficher le panneau Projet**, **Réexécuter le dernier script** (**⌥⌘R**).
- **Clic droit** sur `package.json`, `composer.json`, `Makefile` et autres : **Afficher les scripts**.

![Panneau Projet pour un projet Laravel](images/project-info.png)

> [!NOTE]
> Les scripts susceptibles d’effacer des données (comme `migrate:fresh`, `docker compose down -v`, `rm -rf`, ou ceux dont le nom contient « clean », « reset », « deploy », « publish ») demandent confirmation avant de s’exécuter. Ils sont marqués d’un panneau d’avertissement.

Infos projet nécessite le plug-in Terminal et ne fonctionne que dans les dossiers locaux. Détails : [docs/project-info.md](../../project-info.md).

## Aperçu Markdown {#markdown}

**Aperçu Markdown** affiche le fichier `.md`, `.markdown` ou `.mdx` sélectionné joliment mis en forme dans un panneau latéral à droite. Si rien n’est sélectionné, il affiche le README du dossier.

- Afficher et masquer : **⌥⌘M** (**Afficher/masquer l’aperçu Markdown**).
- Prend en charge le Markdown façon GitHub : tableaux, listes de tâches, encadrés d’alerte, notes de bas de page, code coloré avec un bouton **Copier**, images locales.
- **Sommaire** affiche une table des matières ; les liens vers d’autres fichiers `.md` les ouvrent, **Retour** revient en arrière.
- **Ouvrir dans l’éditeur** et **Afficher dans la sous-fenêtre**.
- En direct : lorsque vous modifiez le fichier, l’aperçu se met à jour et conserve sa position de défilement.

Pour protéger votre vie privée, l’aperçu ne charge pas les images depuis le Web (leur serveur apprendrait que vous avez ouvert le fichier). Pour les activer : **Réglages** → **Plug-ins** → **Aperçu Markdown** → **Charger les images depuis le Web (http/https)**.

Les fichiers de plus de 2 Mo ne sont affichés en partie que sur demande (**Afficher les 2 premiers Mo**). Pas de HTML (sauf les images et les sauts de ligne), pas d’images SVG, pas de formules mathématiques.

## Outils d’image {#bildwerkzeuge}

**Outils d’image** affiche des détails sur les images et modifie de nombreuses images à la fois.

**Pastilles dans la liste :**

- **lo** – basse résolution : plus grand côté inférieur à 800 px.
- **XL** – volumineuse pour le Web : plus de 2 Mo.
- L’infobulle affiche par exemple « 1920×1080 · sRGB · 340 Ko ».

**Panneau Image** (**Afficher le panneau Image** dans la palette) : aperçu, dimensions, proportions, taille du fichier, profil colorimétrique, PPP, couche alpha, animation et données EXIF (appareil photo, objectif, exposition, date de prise de vue). Si une photo contient une position GPS, le panneau vous en avertit et propose **Ouvrir dans Plans**.

**Clic droit → Outils d’image :**

| Commande | Effet |
|---|---|
| **Redimensionner…** | 50 %, 1920/1280/800/400 px ou une taille personnalisée ; conserve les proportions. |
| **Convertir en…** | JPEG, PNG, HEIC, TIFF ; WebP avec `cwebp`. |
| **Faire pivoter de 90° à gauche/à droite**, **Faire pivoter de 180°**, **Retourner horizontalement/verticalement** | |
| **Supprimer les métadonnées…** | Supprime les données EXIF (y compris GPS), XMP, IPTC et les commentaires – sans perte pour JPEG, PNG et WebP. HEIC et TIFF nécessitent `exiftool`. |
| **Optimiser pour le Web…** | Plus grand côté de 2560 px au maximum, sRGB, qualité JPEG 82 (PNG conservé pour la transparence). |
| **Créer un jeu @2x/@1x…** | L’original en @2x et une version @1x de demi-taille. |
| **Copier comme URI de données** | Pour une image unique, à intégrer en HTML/CSS. |

Sous **Enregistrer sous**, vous choisissez **À côté de l’original**, **Dans un sous-dossier** ou **Remplacer les originaux** (les originaux vont dans la corbeille). Manta n’écrase jamais de fichiers existants sans demander ; les noms déjà pris reçoivent `-2`, `-3`…

Outils d’image utilise `sips` de macOS. Pour WebP, installez `cwebp` (`brew install webp`), pour les métadonnées HEIC/TIFF `exiftool` (`brew install exiftool`). Ils ne fonctionnent que dans les dossiers locaux.

![Panneau Image avec les données EXIF](images/image-tools.png)

## Doublons {#duplikate}

**Doublons** trouve les fichiers identiques dans un dossier et ses sous-dossiers et place les copies en trop dans la corbeille.

1. Ouvrez le dossier et choisissez **Rechercher les doublons dans ce dossier** dans la palette de commandes – ou clic droit sur un dossier → **Rechercher les doublons ici**.
2. Manta analyse le dossier en arrière-plan (vous pouvez annuler) et compare la taille, puis une empreinte rapide, puis le SHA-256.
3. Le panneau **Doublons** affiche les groupes, le nombre de fichiers en double et l’espace que vous récupéreriez. Les groupes offrant le plus gros gain apparaissent en premier.
4. Dans chaque groupe, une copie est conservée (« Conservé »). Laquelle est déterminée par la **Règle de conservation** : **Plus ancien**, **Plus récent**, **Chemin le plus court** ou **Dans le dossier…**. Cliquez sur une autre copie pour conserver plutôt celle-ci.
5. **Retirer les copies** (par groupe) ou **Placer les copies dans la corbeille** (toutes) – après une confirmation indiquant le nombre et la taille.

![Panneau Doublons avec des groupes](images/duplicates.png)

Bon à savoir :

- Doublons utilise toujours la **corbeille** de macOS, jamais une suppression définitive. Vous pouvez tout remettre en place.
- Les fichiers modifiés depuis l’analyse ne sont pas touchés.
- `node_modules`, `vendor`, `.git`, les paquets (comme `.app`), les liens symboliques et les liens physiques secondaires sont ignorés.
- **Afficher dans le Finder** et **Sélectionner dans la sous-fenêtre** vous aident à vérifier.
- Options : dans le panneau, dans **Réglages** → **Plug-ins** → **Doublons** (Ignorer les paquets, Inclure les fichiers masqués) et dans `settings.json` (exclusions, taille minimale, commande de corbeille personnalisée).

Doublons ne fonctionne que dans les dossiers locaux.

## Charge du système {#systemauslastung}

**Charge du système** affiche à droite de la barre d’état à quel point votre Mac travaille : « CPU 12 %  GPU 3 %  RAM 12,4 / 16 Go », chaque valeur avec une petite jauge. Les valeurs sont lues toutes les 2 secondes en arrière-plan, sans droits d’administrateur ni programme d’assistance. Tant que la fenêtre est réduite ou masquée, la mesure est suspendue.

- Au-delà de 85 % de charge ou avec une pression sur la mémoire élevée, la valeur et la jauge passent à l’orange ; avec une pression critique, au rouge.
- L’info-bulle donne les détails avec les termes de Moniteur d’activité : les cœurs (cœurs de performance et d’efficacité), le cœur le plus chargé, les cœurs graphiques, la mémoire de l’app, la mémoire résidente, la mémoire compressée, le swap utilisé et la pression sur la mémoire.
- **RAM** correspond à la « Mémoire utilisée » de Moniteur d’activité (mémoire de l’app + résidente + compressée), sans les caches.
- Un clic ouvre **Moniteur d’activité**.
- Dans les fenêtres étroites, le rappel de la palette de commandes disparaît d’abord, puis le GPU ; ensuite, CPU et RAM n’affichent plus que leur icône et leur jauge.

Désactivez l’affichage dans **Réglages** → **Plug-ins** → **Charge du système** ; vous pouvez aussi y décocher **Afficher la charge de la carte graphique**. Sur les Mac sans statistiques de carte graphique, le GPU est omis automatiquement.

## Plug-ins fournis {#mitgeliefert}

**Réglages** → **Plug-ins** → **Disponibles** liste les plug-ins fournis avec Manta mais non installés (l’exemple « Hello Python », par exemple). **Installer** les copie dans votre dossier de plug-ins ; ils démarrent désactivés. Voir [Vos propres plug-ins](15-eigene-plugins.md).
