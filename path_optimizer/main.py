from os import path
from subprocess import check_call
import sys
import glob
from multiprocessing import Pool

# Various manipulations to get a "portable" version of opencv that can go travel to aws lambda
dist_site = path.join(path.curdir, 'dist', 'lib', 'python3.7', 'site-packages')
patchelf = path.join(path.curdir, 'dist', 'bin', 'patchelf')
rpath = path.join(path.abspath(path.curdir), 'dist', 'lib64')
def patch(fname):
    check_call([patchelf, '--set-rpath', rpath, fname])
with Pool(7) as p:
    p.map(patch, [path.join(dist_site, 'cv2.so')] + glob.glob(path.join(rpath, '*.so')))

sys.path.append(dist_site)
from functools import lru_cache
from json import loads, dumps
import numpy as np
import cv2

folder = sys.argv[1]
args = loads(sys.argv[2] if len(sys.argv) > 2 else '{}')
if 'ratio_test' not in args:
    args['ratio_test'] = 0.75
if 'n_features' not in args:
    args['n_features'] = 360
if 'velocity_factor' not in args:
    args['velocity_factor'] = 100

def largest_indices(ary, n):
    """Returns the n largest indices from a numpy array."""
    # https://stackoverflow.com/questions/6910641/how-do-i-get-indices-of-n-maximum-values-in-a-numpy-array
    flat = ary.flatten()
    indices = np.argpartition(flat, -n)[-n:]
    indices = indices[np.argsort(-flat[indices])]
    return np.unravel_index(indices, ary.shape)


_cache = {}
def extract_features(img, cache_key):
    if cache_key in _cache:
        return _cache[cache_key]
    img = img[:-19, :, :]  # perfectly crops out 'google' text
    gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
    gray = np.float32(gray)
    dst = cv2.cornerHarris(gray, 2, 3, 0.04)
    idx = largest_indices(dst, args['n_features'])
    brief = cv2.xfeatures2d.BriefDescriptorExtractor_create()
    kp = [cv2.KeyPoint(float(y), float(x), 1) for x, y in zip(*idx)]
    kp, des = brief.compute(img, kp)
    _cache[cache_key] = (kp, des)
    return kp, des


def get_matching_cost(frame1, frame2, idx1, idx2):
    h, w, _ = frame1.shape

    kp1, des1 = extract_features(frame1, idx1)
    kp2, des2 = extract_features(frame2, idx2)
    diag = np.linalg.norm([[0, 0], [w, h]])

    bf = cv2.BFMatcher()
    matches = bf.knnMatch(des1, des2, k=2)
    matches = [m[0]
               for m in matches if m[0].distance < args['ratio_test'] * m[1].distance]

    frame1_pts = np.float32(
        [kp1[m.queryIdx].pt for m in matches]).reshape(-1, 1, 2)
    frame2_pts = np.float32(
        [kp2[m.trainIdx].pt for m in matches]).reshape(-1, 1, 2)
    # In streetview, frame 2 should be a 'zoomed in' version of frame 1, meaning the homography 2 -> 1 should be in bounds
    try:
        # findHomography throws an error if we have < 4 points
        M, _ = cv2.findHomography(frame2_pts, frame1_pts, cv2.RANSAC)
        center = np.float32([[w//2, h//2]]).reshape(-1, 1, 2)
        # perspectiveTransform can throw an error if M is not full rank (i guess?)
        center_to_frame1 = cv2.perspectiveTransform(center, M)
        frame2_to_frame1 = cv2.perspectiveTransform(frame2_pts, M)
    except Exception:
        return diag*0.5
    cR = np.linalg.norm(frame1_pts - frame2_to_frame1)
    c0 = np.linalg.norm(center - center_to_frame1)
    if cR < 0.5 * diag:
        return min(c0, cR)
    else:
        return 0.5 * diag


@lru_cache(16)
def cached_read(path):
    return cv2.imread(path)

def compute_opt_path(folder):
    img_paths = glob.glob(path.join(folder, '*.jpg'))
    img_paths = sorted(img_paths, key=lambda s: int(
        ''.join([c for c in s if c.isdigit()])))
    n = len(img_paths)
    window_size = 4
    cost = [float('inf')] * n
    prevs = [0] * n
    cost[0] = 0

    for i in range(n-1):
        frame1 = cached_read(img_paths[i])
        for j in range(i+1, min(n, i+window_size+1)):
            frame2 = cached_read(img_paths[j])
            match_cost = get_matching_cost(frame1, frame2, i, j)
            velocity_cost = args['velocity_factor'] * (j - i - 1)**2
            c = match_cost + velocity_cost + cost[i]
            if c < cost[j]:
                cost[j] = c
                prevs[j] = i
    pred = prevs[-1]
    min_path = [n - 1]
    while pred != 0:
        min_path.append(pred)
        pred = prevs[pred]
    min_path.append(0)
    min_path.reverse()
    return min_path

min_path = compute_opt_path(folder)
print(dumps(min_path))
