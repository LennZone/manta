# Vos propres plug-ins {#eigene-plugins}

Vous pouvez enrichir Manta avec vos propres plug-ins – dans n’importe quel langage de programmation. Ce chapitre explique ce qu’un plug-in peut faire, comment l’installer, le mettre à jour et en trouver, et à quoi faire attention. Le guide technique se trouve dans la documentation pour les développeurs.

## Ce que peuvent faire les plug-ins {#moeglichkeiten}

- Afficher des **pastilles** à côté des noms de fichier (comme les pastilles Git).
- Afficher des **pastilles** dans la barre du chemin d’accès (comme la branche ou le projet).
- Proposer des **commandes** pour la palette de commandes et le menu contextuel, avec un raccourci si vous le souhaitez.
- Afficher des **panneaux** sous une sous-fenêtre, dans le panneau latéral droit ou sous forme de zone de dialogue.
- Afficher des **éléments dans la barre d’état**, avec une icône, une petite jauge et une couleur d’alerte (comme [Charge du système](14-plugins.md#systemauslastung)).
- Afficher des **messages**.

Un plug-in est un dossier contenant un fichier `plugin.toml` et un programme (un script Python, par exemple). Manta lance le programme et échange des messages avec lui (lignes JSON via stdin/stdout). Un fichier `.mantaplugin` n’est rien d’autre que ce dossier compressé au format ZIP.

## Installer un plug-in {#installieren}

1. Ouvrez **Réglages** (**⌘,**) → **Plug-ins**.
2. Cliquez sur **Installer un plug-in…** et choisissez **À partir d’un fichier (.mantaplugin, .zip)…** ou **À partir d’un dossier…** (celui qui contient `plugin.toml`). Plus rapide : **double-cliquez** sur un fichier `.mantaplugin` dans le Finder, ou **faites-le glisser** sur la page Plug-ins. Les plug-ins de l’annuaire de la communauté s’installent depuis [Découvrir](#entdecken).
3. Avant toute installation, Manta affiche un récapitulatif : nom, version, auteur, description, **Ce qu’il peut faire** (par exemple « Lit vos fichiers », « Modifie ou supprime des fichiers », « Utilise le réseau », « Lance d’autres programmes », « Exécute des commandes dans le Terminal »), la **Commande** exécutée, la **Source**, un lien vers le **Code source**, la **Licence** et la taille. En dessous, un avertissement rappelle que Manta ne vérifie pas les plug-ins.
4. Cliquez sur **Installer**. Le plug-in apparaît alors sous **Installés** – encore désactivé.

![Récapitulatif avant l’installation d’un plug-in](images/plugin-install.png)

> [!NOTE]
> Les autorisations sont déclarées par l’auteur du plug-in, et Manta les **impose** : le plug-in s’exécute dans un bac à sable macOS et ne peut rien faire d’autre (voir [Sécurité](#sicherheit)).

### L’activer {#einschalten}

Cliquez sur **Activer** sur la carte du plug-in. La première fois, Manta vous demande de confirmer et affiche à nouveau l’auteur, la version, les autorisations, la commande et le dossier dans lequel elle s’exécute.

![Confirmation lors de l’activation d’un plug-in externe](images/plugin-confirm.png)

Si vous avez téléchargé le plug-in sur Internet, macOS place ses fichiers en quarantaine et Gatekeeper bloquerait les programmes qu’il contient. La confirmation le signale : c’est seulement lorsque vous activez le plug-in que Manta retire l’attribut de quarantaine de ses fichiers pour que ses programmes puissent s’exécuter – jamais à votre insu.

Autres boutons sur la carte d’un plug-in : **Recharger**, **Afficher le journal**, **Afficher le dossier**, **Désinstaller**. En haut : **Ouvrir le dossier des plug-ins** et **Recharger** (recherche les plug-ins ajoutés, modifiés ou supprimés).

## Mettre à jour et désinstaller {#aktualisieren}

- **Mettre à jour** : lorsque l’annuaire des plug-ins propose une version plus récente, la carte du plug-in affiche « La version … est disponible dans l’annuaire des plug-ins » avec un bouton **Mettre à jour**. Vous pouvez aussi installer la nouvelle version de la même manière, à partir d’un fichier ou d’un dossier. Si elle a le même identifiant qu’un plug-in installé, le récapitulatif indique une mise à jour et affiche sous **Installé** la version qui sera remplacée. Après confirmation, Manta remplace le plug-in ; son état activé ou désactivé est conservé.
- **Désinstaller** : cliquez sur **Désinstaller** sur la carte du plug-in. Manta supprime le dossier du plug-in ; son dossier de données est conservé.

## Trouver des plug-ins {#entdecken}

Dans **Réglages** → **Plug-ins**, cliquez sur **Découvrir** (ou choisissez **Parcourir les plug-ins…** dans la palette de commandes). Manta affiche les plug-ins de l’annuaire de la communauté : nom et description dans votre langue, auteur, licence, la version de Manta requise, leurs autorisations et un lien vers leur code source.

1. Cliquez sur **Installer** (ou sur **Mettre à jour** pour une version plus récente d’un plug-in installé).
2. Manta télécharge le plug-in et compare sa somme de contrôle SHA-256 avec celle de l’annuaire. Si elles diffèrent, rien n’est installé.
3. Le récapitulatif habituel s’affiche (« Le SHA-256 correspond à l’annuaire des plug-ins ») ; poursuivez comme décrit [plus haut](#installieren).

Les plug-ins qui nécessitent une version plus récente de Manta ou qui ne fonctionnent pas sur votre Mac sont affichés, mais ne peuvent pas être installés. **Sur le web** ouvre l’annuaire sur [manta-filemanager.com/plugins](https://manta-filemanager.com/plugins/).

> [!NOTE]
> **Confidentialité :** Manta charge la liste depuis GitHub uniquement lorsque vous ouvrez **Découvrir** (une fois par session) ou cliquez sur **Actualiser** – jamais en arrière-plan. Il conserve la dernière liste : Découvrir fonctionne donc aussi hors ligne, et les plug-ins installés peuvent proposer des mises à jour sans se connecter à Internet.

## Sécurité {#sicherheit}

Les plug-ins externes s’exécutent dans un **bac à sable macOS** : ils ne peuvent faire que ce que leurs autorisations permettent. Sans autorisation, un plug-in peut seulement lire son propre dossier et le système, et écrire dans son propre dossier de données.

| Autorisation | Permet au plug-in de |
|---|---|
| Lit vos fichiers | lire n’importe quel fichier |
| Modifie ou supprime des fichiers | écrire, créer et supprimer des fichiers n’importe où |
| Utilise le réseau | se connecter à Internet et à votre réseau |
| Lance d’autres programmes | lancer d’autres programmes (qui restent dans le bac à sable) |
| Exécute des commandes dans le Terminal | saisir des commandes dans le Terminal de Manta |

Les mêmes règles s’appliquent à ce qu’un plug-in demande à Manta de faire pour lui (lire un fichier, lister un dossier, renommer des fichiers). La carte du plug-in affiche **Isolé** ; son info-bulle liste les autorisations. Les plug-ins intégrés font partie de Manta et ne sont pas isolés.

Si le bac à sable ne fonctionne pas sur votre Mac, la carte affiche **Non isolé** et la raison, et le récapitulatif indique que les autorisations ne peuvent pas être imposées. Le plug-in s’exécute alors avec les autorisations de votre utilisateur.

> [!WARNING]
> Le bac à sable ne rend pas un plug-in inoffensif : un plug-in autorisé à lire vos fichiers et à utiliser le réseau peut envoyer vos fichiers ailleurs. Manta ne vérifie pas les plug-ins – pas même ceux de l’annuaire des plug-ins. N’installez que des plug-ins de confiance, et lisez le code des plug-ins provenant d’Internet avant de les activer.

C’est pourquoi Manta affiche un récapitulatif avant l’installation, les nouveaux plug-ins démarrent désactivés et Manta vous demande confirmation la première fois que vous en activez un.

## Quand un plug-in ne fonctionne pas {#probleme}

- La carte affiche **Erreur** et la raison, par exemple lorsque `python3` est absent.
- **Afficher le journal** affiche les derniers messages du plug-in.
- Si un plug-in plante, Manta le redémarre – trois fois par minute au maximum. Ensuite, il reste en état d’erreur jusqu’à ce que vous cliquiez sur **Recharger**.
- Si le dossier des plug-ins contient deux dossiers avec le même identifiant de plug-in, le second affiche **Chargement impossible** et un message. Supprimez l’un des deux.

## Emplacement des plug-ins {#speicherorte}

| Quoi | Où |
|---|---|
| Plug-ins installés | `~/Library/Application Support/Manta/plugins/<id>/` |
| Données et journal d’un plug-in | `~/Library/Application Support/Manta/plugin-data/<id>/` (journal : `plugin.log`) |
| État activé/désactivé | `~/Library/Application Support/Manta/plugins.json` |
| Dernière liste de l’annuaire des plug-ins | `~/Library/Application Support/Manta/plugin-registry.json` |
| Règles du bac à sable d’un plug-in | `~/Library/Application Support/Manta/plugin-data/<id>/sandbox.sb` |

La désinstallation supprime le dossier du plug-in ; son dossier de données est conservé.

## Écrire vous-même un plug-in {#schreiben}

Le moyen le plus rapide de commencer est l’exemple **Hello Python** (`examples/plugins/hello-python` dans le code source, commenté ligne par ligne, uniquement avec la bibliothèque standard de Python). Vous pouvez aussi l’installer depuis **Réglages** → **Plug-ins** → **Disponibles**. Il existe aussi des modèles pour Node.js, le shell (bash + jq) et Rust (`examples/plugins/`) ; pour Rust, il y a en plus le crate `manta-plugin-protocol`.

Guide, référence du protocole et exemples : [Écrire des plug-ins pour Manta (docs/plugins.md)](../../plugins.md).

Les plug-ins peuvent parler toutes les langues de Manta : Manta leur indique la langue actuelle, et `plugin.toml` peut contenir un nom et une description traduits (`[localized.fr]`, `[localized.de]`, etc.).

## Développer et tester {#entwickeln}

- **Recharger les plug-ins** (palette de commandes) redémarre tous les plug-ins externes en cours d’exécution et analyse à nouveau le dossier des plug-ins.
- **Mode développeur** : lancez Manta depuis le Terminal avec
  `/Applications/Manta.app/Contents/MacOS/manta --plugin-dev ~/code/my-plugin`
  (répétable, ou via la variable d’environnement `MANTA_PLUGIN_DEV=dir1:dir2`). Pour cette session, le plug-in s’exécute directement depuis ce dossier – approuvé, activé et marqué **Développement**. Une copie installée avec le même identifiant s’efface pendant ce temps ; rien n’est enregistré.
- **Tester sans Manta** : `python3 scripts/plugin-test.py chemin/vers/plugin`.
- **Empaqueter** : `python3 scripts/plugin-pack.py chemin/vers/plugin` crée le fichier `.mantaplugin` et affiche sa somme de contrôle SHA-256 pour l’annuaire.
- **Bac à sable** : déclarez les autorisations dont votre plug-in a besoin – sans elles, il ne peut pas le faire. Si quelque chose est refusé, le journal l’indique (`refused …`).
- **Journal** : **Afficher le journal** sur la carte, ou le fichier `~/Library/Application Support/Manta/plugin-data/<id>/plugin.log`.

Pour en savoir plus : [Develop and debug (docs/plugins.md, en anglais)](../../plugins.md#develop-and-debug).

## Publier un plug-in {#veroeffentlichen}

Placez le plug-in dans un dépôt public sous une licence approuvée par l’OSI, joignez un fichier `.mantaplugin` à une release et ouvrez une pull request qui l’ajoute à `plugins/registry.json`. Il apparaît ensuite dans **Découvrir**. Détails : [Publishing a plugin (docs/plugins.md, en anglais)](../../plugins.md#publishing-a-plugin).
