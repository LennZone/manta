# Mises à jour {#updates}

À chaque lancement, Manta vérifie discrètement si une nouvelle version existe et ne l’installe qu’avec votre accord. Ce chapitre explique comment cela fonctionne, ce qui transite sur le réseau et que faire si une mise à jour échoue.

## Comment vous êtes informé d’une mise à jour {#hinweis}

Lorsqu’une nouvelle version est disponible, vous voyez :

- un message « Mise à jour disponible : 0.2.0 » avec un bouton **Afficher**,
- une petite indication dans la barre d’état (« Mise à jour 0.2.0 »).

Rien ne s’affiche par-dessus votre travail. Si Manta est hors ligne, vous ne remarquez rien ; l’échec est seulement consigné dans le journal.

## Rechercher vous-même les mises à jour {#suchen}

- **Manta** → **Rechercher les mises à jour…** dans la barre des menus, ou
- **Rechercher les mises à jour…** dans la palette de commandes, ou
- **Réglages** → **Général** → **Mises à jour** → **Rechercher**.

Si tout est à jour, Manta indique « Manta est à jour ».

## Installer une mise à jour {#installieren}

1. Cliquez sur **Afficher** dans le message (ou sur l’indication dans la barre d’état). La fenêtre **Mise à jour de logiciels** s’ouvre.
2. Lisez **Nouveautés**. La taille et la date de publication figurent au-dessus.
3. Choisissez :
   - **Installer et relancer** – télécharge la mise à jour, la vérifie, remplace l’app et redémarre Manta.
   - **Plus tard** – vous le rappelle au lancement suivant.
   - **Ignorer cette version** – cette version et les plus anciennes ne sont plus proposées.

![La fenêtre Mise à jour de logiciels](images/software-update.png)

Pendant le téléchargement, vous voyez la progression et pouvez annuler. Si des transferts sont encore en cours, Manta attend qu’ils se terminent avant de redémarrer. macOS peut demander votre mot de passe si Manta ne peut pas écrire dans le dossier **Applications**.

En cas de problème, la fenêtre en indique la raison, avec **Réessayer**. Votre version actuelle reste inchangée.

## Quand Manta ne peut pas se mettre à jour {#blockiert}

| Message | Solution |
|---|---|
| « Manta est exécuté depuis une image disque » | Faites glisser Manta dans le dossier **Applications** et ouvrez-le depuis cet emplacement. |
| « Manta est exécuté depuis un emplacement temporaire » | macOS a lancé Manta depuis une copie en lecture seule (app translocation). Déplacez Manta dans le dossier **Applications** avec le Finder et ouvrez-le depuis cet emplacement. |
| « Les mises à jour ne sont pas configurées dans cette version » | Votre version ne peut pas vérifier les mises à jour. Téléchargez les nouvelles versions depuis le site web. |

## Canaux : Stable et Bêta {#kanaele}

Dans **Réglages** → **Général** → **Mises à jour** → **Canal**, choisissez :

- **Stable** (par défaut) : uniquement les versions finales.
- **Bêta** : également les préversions. Elles proposent les nouvelles fonctionnalités plus tôt, mais peuvent contenir des bogues.

Manta ne propose jamais une version plus ancienne que celle qui est installée.

## Sécurité {#sicherheit}

Chaque mise à jour est signée numériquement. Manta n’installe que ce qui est signé avec la clé de l’équipe Manta et correspond exactement à la version annoncée. Un téléchargement altéré est refusé (« La mise à jour n’est pas correctement signée, rien n’a donc été installé. »). Les mises à jour ne transitent que par HTTPS.

## Confidentialité : ce qu’envoie la recherche de mises à jour {#datenschutz}

Pour vérifier, Manta télécharge deux petits fichiers depuis le serveur de mises à jour (`manta-filemanager.com`) : la liste des versions et sa signature. Ce qui transite sur le réseau :

- votre **adresse IP** (comme pour toute requête web),
- l’identifiant **`Manta-Updater/<version>`** – c’est-à-dire la version de Manta que vous utilisez.

Manta n’envoie **aucun** identifiant d’utilisateur, aucun identifiant d’appareil, aucun nom de fichier et aucune donnée d’utilisation. C’est Manta, sur votre Mac, et non le serveur, qui décide de la version qui vous convient (canal, version de macOS, version ignorée).

Si vous ne le souhaitez pas, désactivez **Rechercher les mises à jour au lancement**. Manta ne se connecte alors à Internet pour les mises à jour que lorsque vous choisissez vous-même **Rechercher les mises à jour…**.

Comme presque tous les sites web, le serveur web consigne ces requêtes avec l’adresse IP. L’hébergeur le fait selon ses propres règles de conservation ; Manta lui-même n’analyse pas ces requêtes et ne les relie à rien.
