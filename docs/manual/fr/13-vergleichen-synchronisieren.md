# Comparer et synchroniser {#vergleichen}

**Comparer et synchroniser** montre en quoi deux dossiers diffèrent et, si vous le souhaitez, les harmonise – sur votre Mac, entre votre Mac et un serveur, ou entre deux serveurs. La fenêtre compare deux sous-fenêtres.

![La fenêtre Comparer et synchroniser](images/compare-sync.png)

## Comparer deux dossiers {#zwei-bereiche}

1. Ouvrez un dossier dans la sous-fenêtre de gauche et l’autre dans la sous-fenêtre de droite.
2. Appuyez sur **⇧⌘S** (**Comparer et synchroniser…**) ou cliquez sur le bouton correspondant de la barre d’outils.
3. Choisissez les options (voir ci-dessous) et cliquez sur **Comparer à nouveau**.

La liste affiche chaque élément avec son état :

| État | Signification |
|---|---|
| **Seulement à gauche** / **Seulement à droite** | N’existe que d’un côté. |
| **Gauche plus récent** / **Droite plus récent** | Des deux côtés, mais l’un est plus récent (date de modification). |
| **Différent** | Même date, mais taille différente. |
| **Identique** | Même date et même taille. |

Manta compare la **date de modification et la taille**, pas le contenu octet par octet.

La légende des états au-dessus de la liste sert aussi de filtre : cliquez sur un état pour le masquer (« Cliquez pour masquer »), cliquez à nouveau pour l’afficher.

## Avec trois sous-fenêtres ou plus {#a-b}

Avec plus de deux sous-fenêtres, vous choisissez les deux à comparer :

- Au lieu de « Gauche » et « Droite », les deux côtés de la fenêtre s’appellent **Sous-fenêtre A** et **Sous-fenêtre B**. Dans le coin supérieur droit de chaque carte, un menu local permet de choisir de quelle sous-fenêtre il s’agit (« Choisissez la sous-fenêtre à comparer »). Les sous-fenêtres sont numérotées de gauche à droite, par exemple « 2 · Téléchargements » ; la sous-fenêtre de l’autre côté est marquée « (A) » ou « (B) ». La choisir échange A et B.
- La sous-fenêtre active et sa [sous-fenêtre cible](02-bereiche-tabs.md#zielbereich) sont présélectionnées.
- Tant que la fenêtre est ouverte, de petits boutons **A**/**B** apparaissent dans la barre du chemin d’accès de chaque sous-fenêtre (« Comparer cette sous-fenêtre comme A »). Le rôle choisi est rempli de couleur.

Les états s’appellent alors **Seulement dans A**, **Seulement dans B**, **A plus récent** et **B plus récent** ; les colonnes de taille **Sous-fenêtre A** et **Sous-fenêtre B**.

![Comparaison avec trois sous-fenêtres : choix de la Sous-fenêtre A et de la Sous-fenêtre B](images/compare-sync-ab.png)

**A est toujours à gauche de la fenêtre, B à droite.** Le sens et les actions gardent donc leur nom : **Copier en miroir de gauche à droite** signifie « de A vers B », « Copier → » copie de A vers B, « Supprimer à droite » supprime dans B.

## Options {#optionen}

- **Inclure les sous-dossiers** – compare aussi le contenu de tous les sous-dossiers.
- **Fichiers masqués** – inclut des fichiers comme `.env` ou `.DS_Store`.
- **Sens** (les trois boutons → ↔ ←) :
  - **→ Copier en miroir de gauche à droite** : le côté droit devient identique au côté gauche.
  - **↔ Le fichier le plus récent l’emporte** : dans les deux sens ; le fichier le plus récent est transféré.
  - **← Copier en miroir de droite à gauche** : le côté gauche devient identique au côté droit.
- **Supprimer les fichiers en trop dans la cible** – uniquement pour la synchronisation unidirectionnelle. Supprime dans la cible ce qui n’existe pas dans la source.

Avant de lancer l’opération, la colonne **Action** indique ce qui va se passer : « Copier → », « ← Copier », « Supprimer à droite » ou « Supprimer à gauche ».

## Synchroniser {#synchronisieren}

1. Comparez les dossiers comme indiqué ci-dessus.
2. Choisissez le sens et vérifiez la colonne **Action**.
3. Cliquez sur **Synchroniser N éléments**.

Les transferts s’exécutent en arrière-plan (« Synchronisation lancée : 12 transferts en attente »). Suivez la progression sous **Transferts** (**⌥⌘L**).

Si les dossiers sont identiques, la fenêtre indique « Les deux dossiers sont synchronisés ».

> [!WARNING]
> La synchronisation **écrase** les fichiers de la cible sans redemander confirmation, et **Supprimer les fichiers en trop dans la cible** supprime définitivement (voir [Supprimer](05-dateien.md#loeschen)). Vérifiez la colonne **Action** avant de commencer.

Règles qui vous protègent :

- Lors d’une copie en miroir, un fichier **plus récent** dans la cible n’est jamais écrasé.
- Avec **Le fichier le plus récent l’emporte**, les fichiers ayant la même date mais une taille différente sont ignorés, car on ne sait pas lequel est le bon. **Supprimer les fichiers en trop dans la cible** ne s’applique pas dans ce cas.
- Si un nom désigne un dossier d’un côté et un fichier de l’autre, il est ignoré.
- Si un dossier ne peut pas être lu, la comparaison s’arrête plutôt que de supprimer les mauvais éléments sur la base de données incomplètes.

## La fenêtre {#fenster}

**Comparer et synchroniser** est une fenêtre macOS à part entière. Fermez-la avec le bouton rouge de la fenêtre ou **⌘W** (pas avec esc). Manta mémorise sa position tant que l’app est ouverte.

Avec une seule sous-fenêtre, la commande est désactivée (« Nécessite deux sous-fenêtres – ⌘D divise la sous-fenêtre »).
