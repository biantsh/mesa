"""Train a classifier on the Iris dataset and plot how well it does."""

from pathlib import Path

import matplotlib.pyplot as plt
import numpy as np
from sklearn.datasets import load_iris
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import accuracy_score, confusion_matrix
from sklearn.model_selection import learning_curve, train_test_split
from sklearn.pipeline import make_pipeline
from sklearn.preprocessing import StandardScaler

FIGURES = Path(__file__).parent / "figures"
SEED = 9


def train(X_train, y_train):
    model = make_pipeline(StandardScaler(), LogisticRegression(max_iter=1000))
    model.fit(X_train, y_train)
    return model


def plot_confusion_matrix(cm, class_names):
    fig, ax = plt.subplots(figsize=(5.2, 4.4))
    im = ax.imshow(cm, cmap="Blues")
    fig.colorbar(im, ax=ax, fraction=0.046, pad=0.04)

    ticks = np.arange(len(class_names))
    ax.set_xticks(ticks, labels=class_names)
    ax.set_yticks(ticks, labels=class_names)
    ax.set_xlabel("Predicted label")
    ax.set_ylabel("True label")
    ax.set_title("Confusion matrix: logistic regression")

    # Write each count in its cell, white on the dark ones.
    threshold = cm.max() / 2
    for i in range(cm.shape[0]):
        for j in range(cm.shape[1]):
            color = "white" if cm[i, j] > threshold else "black"
            ax.text(j, i, cm[i, j], ha="center", va="center", color=color, fontsize=13)

    fig.tight_layout()
    return fig


def plot_learning_curve(model, X, y):
    sizes, train_scores, test_scores = learning_curve(
        model, X, y, cv=5, train_sizes=np.linspace(0.2, 1.0, 6), shuffle=True, random_state=SEED
    )
    fig, ax = plt.subplots(figsize=(5.2, 4.4))
    ax.plot(sizes, train_scores.mean(axis=1), "o-", label="Training")
    ax.plot(sizes, test_scores.mean(axis=1), "o-", label="Cross-validation")
    ax.set_xlabel("Training examples")
    ax.set_ylabel("Accuracy")
    ax.set_title("Learning curve")
    ax.legend(loc="lower right")
    ax.grid(alpha=0.3)
    fig.tight_layout()
    return fig


def main():
    iris = load_iris()
    X_train, X_test, y_train, y_test = train_test_split(
        iris.data, iris.target, test_size=0.3, stratify=iris.target, random_state=SEED
    )
    model = train(X_train, y_train)
    predicted = model.predict(X_test)
    print(f"Accuracy: {accuracy_score(y_test, predicted):.3f}")

    FIGURES.mkdir(exist_ok=True)
    cm = confusion_matrix(y_test, predicted)
    plot_confusion_matrix(cm, iris.target_names).savefig(FIGURES / "confusion_matrix.png", dpi=150)
    plot_learning_curve(model, iris.data, iris.target).savefig(FIGURES / "learning_curve.png", dpi=150)


if __name__ == "__main__":
    main()
