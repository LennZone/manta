# Favoris et barre latérale {#seitenleiste}

La barre latérale, à gauche, vous mène en un clic à vos emplacements les plus importants : favoris, dossiers standard, disques, serveurs connectés, conteneurs et stockage cloud. Un clic ouvre l’emplacement dans la sous-fenêtre active.

![Barre latérale avec toutes ses sections](images/sidebar.png)

## Les sections {#abschnitte}

| Section | Contenu |
|---|---|
| **Favoris** | Les dossiers et connexions aux serveurs que vous avez conservés. |
| **Emplacements** | Départ, Applications, Bureau, Documents, Téléchargements et, tout en bas, la **Corbeille** (voir ci-dessous). |
| **Cloud** | iCloud Drive et les dossiers des apps cloud (Google Drive pour ordinateur, OneDrive, Dropbox, Box…), voir [Stockage cloud](09-cloud.md). La section n’apparaît que si Manta trouve au moins un stockage cloud. |
| **Appareils** | Macintosh HD et tous les volumes montés (clés USB, disques, partages réseau, images disque). |
| **Serveurs** | Les serveurs auxquels vous êtes connecté. Sans connexion, la section affiche **Se connecter au serveur…**. |
| **Conteneurs** | Les conteneurs Docker en cours d’exécution et arrêtés, ainsi que les pods Kubernetes (uniquement si Docker est installé ou si un contexte Kubernetes est défini). |

Le bouton en forme de flèche circulaire à côté de **Cloud** et **Appareils** recherche à nouveau les dossiers cloud et les volumes. Manta repère aussi de lui-même les nouvelles apps cloud.

Survolez un emplacement pour voir son chemin complet dans une infobulle.

## Ajouter un dossier aux Favoris {#favorit-hinzufuegen}

Il existe trois façons de procéder :

- **Clavier** : ouvrez le dossier dans la sous-fenêtre active et appuyez sur **⌃⌘T** (**Ajouter le dossier aux Favoris**).
- **Souris** : cliquez sur le **+** à côté de l’en-tête **Favoris** (« Ajouter le dossier actuel »), ou choisissez **Ajouter le dossier aux Favoris** dans le menu contextuel d’un emplacement vide de la sous-fenêtre.
- **Glisser** : faites glisser un ou plusieurs dossiers d’une sous-fenêtre sur la section **Favoris** (voir ci-dessous).

Les dossiers **situés dans des archives** ne peuvent pas être des favoris (« Les dossiers situés dans des archives ne peuvent pas être ajoutés aux Favoris »).

### Faire glisser des dossiers sur les Favoris {#favoriten-ziehen}

1. Sélectionnez un ou plusieurs dossiers dans une sous-fenêtre – dans n’importe quelle présentation, en local ou sur un serveur.
2. Faites-les glisser sur la section **Favoris**. La section est mise en évidence, une ligne indique où les dossiers seront placés, et le pointeur affiche par exemple « Ajouter 2 dossiers aux Favoris ».
3. Relâchez. Manta indique « « Projets » ajouté aux Favoris » ou « 2 dossiers ajoutés aux Favoris ».

Bon à savoir :

- **Dossiers uniquement** : si vous ne faites glisser que des fichiers, le pointeur affiche « Seuls les dossiers peuvent être des favoris » et rien ne se passe. Avec une sélection mixte, Manta ne prend que les dossiers.
- **Pas de doublons** : si un dossier est déjà un favori, l’entrée existante clignote brièvement et Manta indique « « nom » figure déjà dans les Favoris ». Seul l’emplacement compte, pas le nom du favori.
- **Dossiers de serveur** : faire glisser un dossier depuis un serveur crée un favori de serveur avec ce dossier comme dossier de départ, par exemple « files.example.com – www ». Aucun nouveau mot de passe n’est enregistré avec lui ; si le trousseau en contient déjà un pour ce compte sur ce serveur (provenant d’un autre favori), le nouveau favori l’utilise aussi. Sinon, Manta le demande à l’ouverture (sauf si vous vous identifiez avec ssh-agent ou un fichier de clé sans phrase de passe).

![Un dossier glissé sur les Favoris](images/favorites-drag.png)

## Réorganiser et supprimer des favoris {#favoriten-ordnen}

- **Réorganiser** : faites glisser un favori vers le haut ou vers le bas dans la liste. La ligne déplacée s’estompe et une ligne indique la nouvelle position. L’ordre est enregistré immédiatement.
- **Supprimer** : clic droit sur le favori → **Supprimer des Favoris**, ou faites glisser le favori sur la **Corbeille** dans la barre latérale. Le dossier lui-même est bien sûr conservé.

## Corbeille {#papierkorb}

La **Corbeille** est la dernière ligne sous **Emplacements**, comme dans le Finder. Son icône indique si elle est vide ou pleine (une corbeille remplie = elle contient quelque chose) ; l’infobulle donne le nombre d’éléments.

- **Ouvrir** : cliquez sur **Corbeille**. Elle s’ouvre dans la sous-fenêtre active comme n’importe quel dossier. **Supprimer immédiatement…** (ou **⌘⌫**) y supprime définitivement des éléments individuels – Manta demande d’abord confirmation.
- **Vider** : clic droit sur **Corbeille** → **Vider la corbeille…**, ou appuyez sur **⇧⌘⌫**, ou choisissez **Vider la corbeille…** dans la palette de commandes. Manta demande (« Voulez-vous vraiment supprimer définitivement les éléments de la corbeille ? ») et affiche le nombre d’éléments et leur taille. Cette opération est irréversible. Les éléments qui ne peuvent pas être supprimés (les éléments verrouillés, par exemple) sont ignorés et signalés.
- **Faire glisser dessus** : faites glisser des éléments d’une sous-fenêtre sur **Corbeille**. La ligne est mise en évidence et le pointeur affiche par exemple « Placer 3 éléments dans la corbeille ». Les éléments locaux sont placés dans la corbeille sans confirmation (comme avec **⌘⌫**). Les éléments situés sur un **serveur**, dans **S3** ou dans un **conteneur** ne peuvent pas aller dans une corbeille : Manta vous demande s’il faut les supprimer définitivement. Les éléments d’une **archive** ne peuvent pas être supprimés – le pointeur l’indique. Vous pouvez aussi faire glisser sur la corbeille des fichiers provenant d’autres apps (le Finder, par exemple).
- **Remettre** : ce que vous venez de placer dans la corbeille dans Manta revient avec **⌘Z** (**Annuler**), voir [Annuler](05-dateien.md#widerrufen). Pour les éléments plus anciens, passez par le Finder (clic droit → **Ouvrir dans le Finder**, puis **Remettre**) : le Finder conserve l’emplacement d’origine dans ses propres données, que Manta ne peut pas lire.

### Accès complet au disque {#papierkorb-zugriff}

macOS protège la corbeille : pour en afficher le contenu, Manta a besoin de l’**Accès complet au disque**. Sans celui-ci, la sous-fenêtre affiche « Manta a besoin de l’accès complet au disque pour afficher la corbeille » et propose deux boutons :

1. **Ouvrir Accès complet au disque** ouvre **Réglages Système › Confidentialité et sécurité › Accès complet au disque**. Activez **Manta** à cet endroit, puis rouvrez la corbeille (**⌘R**). Il faut parfois redémarrer Manta pour que cela prenne effet.
2. **Ouvrir dans le Finder** affiche la corbeille dans le Finder – même si Manta est votre gestionnaire de fichiers par défaut.

Sans Accès complet au disque, le glisser-déposer sur la corbeille et **⌘⌫** fonctionnent toujours, et la ligne indique toujours si la corbeille est vide ou pleine. **Vider la corbeille…** est alors effectué par le Finder (macOS demande une fois si Manta peut contrôler le Finder) ; le Finder vide aussi la corbeille des autres volumes.

## Les serveurs en favoris {#server-favoriten}

Une connexion à un serveur devient un favori lorsque vous activez **Ajouter aux Favoris** dans la zone de dialogue **Se connecter au serveur**. Vous pouvez lui donner un nom personnalisé (« Nom dans la barre latérale (facultatif) ») et un dossier de départ.

Clic droit sur un favori de serveur :

- **Ouvrir** – se connecte et ouvre le dossier de départ.
- **Modifier la connexion…** – ouvre la zone de dialogue de connexion avec les informations enregistrées.
- **Supprimer des Favoris**.

Appuyer sur **⌃⌘T** dans un dossier situé sur un serveur crée un favori de serveur avec ce dossier comme dossier de départ. Le mot de passe n’est pas copié.

Voir [Connexions](08-verbindungen.md#favoriten).

## Disques {#geraete}

Les disques nouvellement connectés et les images disque ouvertes (par exemple un DMG à partir duquel vous installez une app) apparaissent d’eux-mêmes dans la barre latérale : les volumes internes et les partages réseau sous **Appareils**, les disques éjectables sous **Éjectables** (voir ci-dessous). Si l’un d’eux n’apparaît pas, cliquez sur l’icône de flèche à côté de **Appareils** (**Analyser à nouveau les volumes**).

Comme dans le Finder, les images disque, clés USB et disques externes sont listés dans leur propre section, **Éjectables**, avec l’icône propre du volume (ou celle de l’app qu’il contient) s’il en a une. L’icône d’éjection à côté éjecte le disque. Le menu contextuel propose aussi **Ouvrir**, **Ouvrir dans un nouvel onglet**, **Afficher dans le dossier parent** (pour une image disque, le fichier DMG) et **Éjecter « Nom »**. Les sous-fenêtres qui affichent le disque passent à **Départ** lors de l’éjection. Si une app utilise encore le disque, Manta indique qu’il n’a pas pu être éjecté.

## Serveurs connectés {#server}

Chaque connexion ouverte est listée sous **Serveurs** avec un point vert. Un clic l’ouvre dans la sous-fenêtre active. L’icône d’éjection qui apparaît au survol ferme la connexion (**Déconnecter**).

## Conteneurs {#container}

La section **Conteneurs** liste vos conteneurs Docker, regroupés par projet Compose comme dans Docker Desktop. Les conteneurs en cours d’exécution ont un point coloré ; les conteneurs arrêtés sont estompés et repliés sous « Arrêtés (N) ». Pour le contexte Kubernetes actuel, un sélecteur d’espace de noms affiche les pods et leurs conteneurs.

**Démarrer, redémarrer, arrêter** : cliquez avec le bouton droit sur un conteneur Docker. Un conteneur arrêté propose **Démarrer**, un conteneur en cours d’exécution **Redémarrer** et **Arrêter**. Manta exécute `docker start`, `docker restart` ou `docker stop` ; pendant ce temps, la ligne indique « Démarrage… » (etc.) et un message confirme le résultat. Si un conteneur s’arrête alors qu’il est ouvert dans un panneau, celui-ci propose **Démarrer le conteneur** au lieu de **Réessayer**, puis réaffiche le dossier une fois le conteneur lancé.

La liste s’actualise en arrière-plan toutes les 5 secondes. L’icône de flèche (**Actualiser les conteneurs**) la relit immédiatement. Voir [Connexions](08-verbindungen.md#container).

## Emplacement de stockage des favoris {#speicherort}

Dans `~/Library/Application Support/Manta/favorites.json`. Les mots de passe n’y figurent pas : avec **Mémoriser le mot de passe**, ils sont placés dans le trousseau macOS, voir [Connexions](08-verbindungen.md#favoriten).
